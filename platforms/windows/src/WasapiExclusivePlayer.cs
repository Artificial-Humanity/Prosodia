using System;
using System.Collections.Concurrent;
using System.Runtime.InteropServices;
using System.Threading;

namespace Prosodia.Platforms.Windows
{
    public class WasapiExclusivePlayer : IDisposable
    {
        private const uint AUDCLNT_STREAMFLAGS_EVENTCALLBACK = 0x00040000;
        private const uint AUDCLNT_SHAREMODE_EXCLUSIVE = 1;
        
        private IMMDeviceEnumerator _deviceEnumerator;
        private IMMDevice _device;
        private IAudioClient _audioClient;
        private IAudioRenderClient _renderClient;
        
        private Thread _renderThread;
        private AutoResetEvent _bufferEvent;
        private bool _isDisposed;
        private bool _isPlaying;
        
        private readonly ConcurrentQueue<float[]> _audioQueue = new ConcurrentQueue<float[]>();
        private float[] _currentBuffer;
        private int _currentBufferOffset;
        
        private int _sampleRate;
        private int _channels;
        private int _bufferFrameCount;

        [StructLayout(LayoutKind.Sequential)]
        private struct RustCallStatus
        {
            public byte code;
            public uint error_len;
            public IntPtr error_buf;
        }

        [DllImport("stage", CallingConvention = CallingConvention.Cdecl)]
        private static extern uint uniffi_stage_fn_func_get_sample_rate(ref RustCallStatus status);

        private static uint GetStageSampleRate()
        {
            try
            {
                var status = new RustCallStatus();
                return uniffi_stage_fn_func_get_sample_rate(ref status);
            }
            catch
            {
                return 24000;
            }
        }

        public WasapiExclusivePlayer(int sampleRate = -1, int channels = 1)
        {
            _sampleRate = sampleRate == -1 ? (int)GetStageSampleRate() : sampleRate;
            _channels = channels;
            InitializeWasapi();
        }

        private static void CheckHr(int hr)
        {
            if (hr < 0)
            {
                Marshal.ThrowExceptionForHR(hr);
            }
        }

        public void EnqueueSamples(float[] samples)
        {
            if (samples == null || samples.Length == 0) return;
            _audioQueue.Enqueue(samples);
        }

        public void Play()
        {
            if (_isPlaying) return;
            _isPlaying = true;

            // Pre-fill buffer with silence to kick-start clock event and avoid glitching
            IntPtr bufferPtr;
            CheckHr(_renderClient.GetBuffer((uint)_bufferFrameCount, out bufferPtr));
            var silence = new byte[_bufferFrameCount * _channels * 4];
            Marshal.Copy(silence, 0, bufferPtr, silence.Length);
            CheckHr(_renderClient.ReleaseBuffer((uint)_bufferFrameCount, 0));
            
            CheckHr(_audioClient.Start());
            
            _renderThread = new Thread(RenderLoop)
            {
                Name = "ProsodiaWASAPIRenderThread",
                Priority = ThreadPriority.Highest,
                IsBackground = true
            };
            _renderThread.Start();
        }

        public void Stop()
        {
            if (!_isPlaying) return;
            _isPlaying = false;
            
            CheckHr(_audioClient.Stop());
            _renderThread?.Join();
            CheckHr(_audioClient.Reset());

            _currentBuffer = null;
            _currentBufferOffset = 0;
            while (_audioQueue.TryDequeue(out _)) { }
        }

        private void InitializeWasapi()
        {
            // Create Device Enumerator COM class
            var enumeratorType = Type.GetTypeFromCLSID(new Guid("BCDE0395-E52F-467C-8E3D-C4579291692E"));
            _deviceEnumerator = (IMMDeviceEnumerator)Activator.CreateInstance(enumeratorType);
            
            // Get default playback endpoint
            CheckHr(_deviceEnumerator.GetDefaultAudioEndpoint(0, 1, out _device)); // 0 = eRender, 1 = eMultimedia
            
            // Activate AudioClient
            var audioClientGuid = new Guid("1CB9AD4C-DBA0-4C15-9C8A-42558517A7D8");
            CheckHr(_device.Activate(ref audioClientGuid, 23, IntPtr.Zero, out var clientObj)); // 23 = CLSCTX_ALL
            _audioClient = (IAudioClient)clientObj;

            // Formulate wave format (IEEE float)
            var wfx = new WaveFormatExtensible(_sampleRate, 32, _channels);
            var wfxPtr = Marshal.AllocHGlobal(Marshal.SizeOf(wfx));
            Marshal.StructureToPtr(wfx, wfxPtr, false);

            try
            {
                // In Exclusive mode, we must query if the device supports the exact format
                int hr = _audioClient.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, wfxPtr, out var closestMatch);
                if (closestMatch != IntPtr.Zero)
                {
                    Marshal.FreeCoTaskMem(closestMatch);
                    closestMatch = IntPtr.Zero;
                }

                if (hr != 0)
                {
                    // Fall back to typical 48kHz stereo float if 24kHz mono is rejected by exclusive driver
                    _sampleRate = 48000;
                    _channels = 2;
                    wfx = new WaveFormatExtensible(_sampleRate, 32, _channels);
                    Marshal.StructureToPtr(wfx, wfxPtr, false);
                    hr = _audioClient.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, wfxPtr, out closestMatch);
                    if (closestMatch != IntPtr.Zero)
                    {
                        Marshal.FreeCoTaskMem(closestMatch);
                        closestMatch = IntPtr.Zero;
                    }
                    if (hr != 0)
                    {
                        throw new NotSupportedException("Device does not support exclusive mode with IEEE float formats.");
                    }
                }

                // Query recommended device period (typically 3-10ms)
                CheckHr(_audioClient.GetDevicePeriod(out var defaultPeriod, out var minimumPeriod));
                
                // Initialize in Exclusive, Event-Driven mode
                hr = _audioClient.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, minimumPeriod, minimumPeriod, wfxPtr, Guid.Empty);
                if (hr == unchecked((int)0x88890019)) // AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED
                {
                    CheckHr(_audioClient.GetBufferSize(out var alignedFrameCount));
                    long newBufferDuration = (long)(10000000.0 * alignedFrameCount / _sampleRate);
                    
                    Marshal.ReleaseComObject(_audioClient);
                    _audioClient = null;
                    
                    CheckHr(_device.Activate(ref audioClientGuid, 23, IntPtr.Zero, out clientObj));
                    _audioClient = (IAudioClient)clientObj;
                    
                    hr = _audioClient.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, newBufferDuration, newBufferDuration, wfxPtr, Guid.Empty);
                }
                CheckHr(hr);
                
                // Retrieve actual buffer size (in frames)
                CheckHr(_audioClient.GetBufferSize(out var bufferSize));
                _bufferFrameCount = (int)bufferSize;
                
                // Register event handler callback
                _bufferEvent = new AutoResetEvent(false);
                CheckHr(_audioClient.SetEventHandle(_bufferEvent.SafeWaitHandle.DangerousGetHandle()));
                
                // Get Render Client
                var renderClientGuid = new Guid("F294ACFC-3146-4483-A7BF-ADDCA7C260E2");
                CheckHr(_audioClient.GetService(ref renderClientGuid, out var renderObj));
                _renderClient = (IAudioRenderClient)renderObj;
            }
            finally
            {
                Marshal.FreeHGlobal(wfxPtr);
            }
        }

        private void RenderLoop()
        {
            while (_isPlaying)
            {
                // Wait for WASAPI hardware buffer request event
                if (!_bufferEvent.WaitOne(500)) continue;
                if (!_isPlaying) break;

                IntPtr bufferPtr;
                CheckHr(_renderClient.GetBuffer((uint)_bufferFrameCount, out bufferPtr));
                
                int totalSamplesNeeded = _bufferFrameCount * _channels;
                float[] renderBuffer = new float[totalSamplesNeeded];
                int samplesWritten = 0;

                while (samplesWritten < totalSamplesNeeded)
                {
                    if (_currentBuffer == null || _currentBufferOffset >= _currentBuffer.Length)
                    {
                        if (_audioQueue.TryDequeue(out var nextBuffer))
                        {
                            if (_sampleRate == 48000 && _channels == 2)
                            {
                                // Resample 24 kHz mono to 48 kHz stereo on the fly
                                float[] converted = new float[nextBuffer.Length * 4];
                                for (int i = 0; i < nextBuffer.Length; i++)
                                {
                                    float current = nextBuffer[i];
                                    float next = (i + 1 < nextBuffer.Length) ? nextBuffer[i + 1] : current;

                                    // Frame 1 (48 kHz sample 1): Left/Right copy
                                    converted[i * 4 + 0] = current;
                                    converted[i * 4 + 1] = current;

                                    // Frame 2 (48 kHz sample 2): Left/Right linear interpolation
                                    float interp = (current + next) * 0.5f;
                                    converted[i * 4 + 2] = interp;
                                    converted[i * 4 + 3] = interp;
                                }
                                _currentBuffer = converted;
                            }
                            else
                            {
                                _currentBuffer = nextBuffer;
                            }
                            _currentBufferOffset = 0;
                        }
                        else
                        {
                            // Output silence if queue is starved
                            break;
                        }
                    }

                    int copyCount = Math.Min(totalSamplesNeeded - samplesWritten, _currentBuffer.Length - _currentBufferOffset);
                    Array.Copy(_currentBuffer, _currentBufferOffset, renderBuffer, samplesWritten, copyCount);
                    _currentBufferOffset += copyCount;
                    samplesWritten += copyCount;
                }

                // Copy float buffer into unmanaged COM audio buffer
                Marshal.Copy(renderBuffer, 0, bufferPtr, totalSamplesNeeded);
                CheckHr(_renderClient.ReleaseBuffer((uint)_bufferFrameCount, 0));
            }
        }

        public void Dispose()
        {
            if (_isDisposed) return;
            _isDisposed = true;
            Stop();
            
            _bufferEvent?.Dispose();
            if (_renderClient != null) Marshal.ReleaseComObject(_renderClient);
            if (_audioClient != null) Marshal.ReleaseComObject(_audioClient);
            if (_device != null) Marshal.ReleaseComObject(_device);
            if (_deviceEnumerator != null) Marshal.ReleaseComObject(_deviceEnumerator);
        }

        // --- WASAPI / COM Interop Structs & Interfaces ---

        [StructLayout(LayoutKind.Sequential, Pack = 1)]
        private struct WaveFormatExtensible
        {
            public ushort wFormatTag;
            public ushort nChannels;
            public uint nSamplesPerSec;
            public uint nAvgBytesPerSec;
            public ushort nBlockAlign;
            public ushort wBitsPerSample;
            public ushort cbSize;
            public ushort wValidBitsPerSample;
            public uint dwChannelMask;
            public Guid SubFormat;

            public WaveFormatExtensible(int rate, int bits, int channels)
            {
                wFormatTag = 0xFFFE; // WAVE_FORMAT_EXTENSIBLE
                nChannels = (ushort)channels;
                nSamplesPerSec = (uint)rate;
                wBitsPerSample = (ushort)bits;
                cbSize = 22;
                wValidBitsPerSample = (ushort)bits;
                dwChannelMask = channels == 1 ? 4U : 3U; // Mono (FC) or Stereo (FL|FR)
                SubFormat = new Guid("00000003-0000-0010-8000-00aa00389b71"); // KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
                nBlockAlign = (ushort)(nChannels * (wBitsPerSample / 8));
                nAvgBytesPerSec = nSamplesPerSec * nBlockAlign;
            }
        }

        [ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
        private interface IMMDeviceEnumerator
        {
            [PreserveSig]
            int EnumAudioEndpoints(int dataFlow, int stateMask, out IntPtr devices);
            [PreserveSig]
            int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice endpoint);
            [PreserveSig]
            int GetDevice([MarshalAs(UnmanagedType.LPWStr)] string pwstrId, out IMMDevice device);
        }

        [ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
        private interface IMMDevice
        {
            [PreserveSig]
            int Activate(ref Guid iid, uint dwClsCtx, IntPtr pActivationParams, [MarshalAs(UnmanagedType.IUnknown)] out object ppInterface);
            [PreserveSig]
            int OpenPropertyStore(uint stgmAccess, out IntPtr properties);
            [PreserveSig]
            int GetId([MarshalAs(UnmanagedType.LPWStr)] out string ppstrId);
            [PreserveSig]
            int GetState(out uint pdwState);
        }

        [ComImport, Guid("1CB9AD4C-DBA0-4C15-9C8A-42558517A7D8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
        private interface IAudioClient
        {
            [PreserveSig]
            int Initialize(uint shareMode, uint streamFlags, long hnsBufferDuration, long hnsPeriodicity, IntPtr pFormat, [MarshalAs(UnmanagedType.LPStruct)] Guid audioSessionGuid);
            [PreserveSig]
            int GetBufferSize(out uint pNumBufferFrames);
            [PreserveSig]
            int GetStreamLatency(out long phnsLatency);
            [PreserveSig]
            int GetCurrentPadding(out uint pNumPaddingFrames);
            [PreserveSig]
            int IsFormatSupported(uint shareMode, IntPtr pFormat, out IntPtr ppClosestMatch);
            [PreserveSig]
            int GetMixFormat(out IntPtr ppDeviceFormat);
            [PreserveSig]
            int GetDevicePeriod(out long phnsDefaultDevicePeriod, out long phnsMinimumDevicePeriod);
            [PreserveSig]
            int Start();
            [PreserveSig]
            int Stop();
            [PreserveSig]
            int Reset();
            [PreserveSig]
            int SetEventHandle(IntPtr eventHandle);
            [PreserveSig]
            int GetService(ref Guid riid, [MarshalAs(UnmanagedType.IUnknown)] out object ppv);
        }

        [ComImport, Guid("F294ACFC-3146-4483-A7BF-ADDCA7C260E2"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
        private interface IAudioRenderClient
        {
            [PreserveSig]
            int GetBuffer(uint numFramesRequested, out IntPtr ppData);
            [PreserveSig]
            int ReleaseBuffer(uint numFramesWritten, uint dwFlags);
        }
    }
}
