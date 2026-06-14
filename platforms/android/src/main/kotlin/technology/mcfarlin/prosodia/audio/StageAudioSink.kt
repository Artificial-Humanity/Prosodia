package technology.mcfarlin.prosodia.audio

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Shared Android AudioTrack output stage for Actor renderers.
 */
class StageAudioSink(private val sampleRate: Int = 24000) {
    private var audioTrack: AudioTrack? = null
    private val format = AudioFormat.Builder()
        .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
        .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
        .setSampleRate(sampleRate)
        .build()

    private val attributes = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
        .build()

    private var connected = false
    private var paused = false
    private var totalSamplesCompleted: Long = 0

    @Synchronized
    private fun initAudioTrack() {
        if (connected) return
        val bufferSize = AudioTrack.getMinBufferSize(
            sampleRate,
            AudioFormat.CHANNEL_OUT_MONO,
            AudioFormat.ENCODING_PCM_FLOAT
        ) * 2

        audioTrack = AudioTrack.Builder()
            .setAudioAttributes(attributes)
            .setAudioFormat(format)
            .setBufferSizeInBytes(bufferSize)
            .setTransferMode(AudioTrack.MODE_STREAM)
            .build()

        audioTrack?.play()
        connected = true
        paused = false
    }

    suspend fun scheduleSilence(seconds: Double) {
        val frames = (seconds * sampleRate).toInt()
        if (frames <= 0) return
        schedule(FloatArray(frames))
    }

    suspend fun schedule(samples: FloatArray) = withContext(Dispatchers.IO) {
        if (samples.isEmpty()) return@withContext

        synchronized(this@StageAudioSink) {
            if (!connected) {
                initAudioTrack()
            }
        }

        val track = audioTrack ?: return@withContext

        // Block writing to AudioTrack to provide natural backpressure when output buffer is full
        track.write(samples, 0, samples.size, AudioTrack.WRITE_BLOCKING)

        synchronized(this@StageAudioSink) {
            totalSamplesCompleted += samples.size
        }
    }

    @Synchronized
    fun pause() {
        if (connected && !paused) {
            audioTrack?.pause()
            paused = true
        }
    }

    @Synchronized
    fun resume() {
        if (connected && paused) {
            audioTrack?.play()
            paused = false
        }
    }

    @Synchronized
    fun stop() {
        audioTrack?.let {
            it.stop()
            it.release()
        }
        audioTrack = null
        connected = false
        paused = false
        totalSamplesCompleted = 0
    }

    @Synchronized
    fun getTotalSamplesCompleted(): Long {
        return totalSamplesCompleted
    }
}
