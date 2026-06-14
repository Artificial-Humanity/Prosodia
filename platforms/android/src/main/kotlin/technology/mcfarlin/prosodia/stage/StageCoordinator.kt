package technology.mcfarlin.prosodia.stage

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import technology.mcfarlin.prosodia.audio.StageAudioSink

// MARK: - NarrationSourceAdapter

class NarrationSourceAdapter(
    private val document: BookDocument
) : uniffi.stage.NarrationSource {
    private var currentIndex = 0

    override fun nextPassage(): String? {
        if (currentIndex >= document.chapterCount) return null
        return runBlocking {
            try {
                val chapter = document.chapter(currentIndex)
                currentIndex++
                chapter.text
            } catch (e: Exception) {
                null
            }
        }
    }
}

// MARK: - StageCoordinator

object StageCoordinator {
    fun run(
        document: BookDocument,
        grouping: uniffi.stage.NarrationGrouping = uniffi.stage.NarrationGrouping.Sentence,
        director: uniffi.stage.DirectorInference,
        actor: uniffi.stage.VocalActor
    ): PlaybackController {
        val sourceAdapter = NarrationSourceAdapter(document)
        val rustCoordinator = uniffi.stage.StageCoordinator(
            source = sourceAdapter,
            director = director,
            actor = actor,
            grouping = grouping,
            sampleRate = 24000
        )

        val audioSink = StageAudioSink(sampleRate = 24000)
        val controller = RustCoordinatedPlaybackController(rustCoordinator, audioSink)
        controller.start()
        return controller
    }
}

// MARK: - RustCoordinatedPlaybackController

class RustCoordinatedPlaybackController(
    private val coordinator: uniffi.stage.StageCoordinator,
    private val audioSink: StageAudioSink
) : PlaybackController {

    private val _events = MutableSharedFlow<PlaybackEvent>(replay = 0)
    override val events: Flow<PlaybackEvent> = _events.asSharedFlow()

    private val scope = CoroutineScope(Dispatchers.Default + SupervisorJob())
    private var driveJob: Job? = null
    private var paused = false

    fun start() {
        driveJob = scope.launch {
            while (isActive) {
                if (paused) {
                    delay(50)
                    continue
                }

                // Pull the next chunk off the main thread since nextChunk blocks on Director/Actor inference
                val chunk = withContext(Dispatchers.IO) {
                    if (!paused && isActive) {
                        coordinator.nextChunk()
                    } else {
                        null
                    }
                }

                if (chunk == null) {
                    _events.emit(PlaybackEvent.Finished)
                    break
                }

                _events.emit(PlaybackEvent.SentenceBegan(chunk.index.toInt()))

                try {
                    // Feed floating PCM array to the AudioSink
                    audioSink.schedule(chunk.audio.toFloatArray())
                    _events.emit(PlaybackEvent.SentenceScheduled(chunk.index.toInt(), emptyList()))
                } catch (e: Exception) {
                    _events.emit(PlaybackEvent.SegmentFailed(chunk.index.toInt(), e))
                }
            }
        }
    }

    override suspend fun pause() {
        paused = true
        audioSink.pause()
    }

    override suspend fun resume() {
        paused = false
        audioSink.resume()
    }

    override suspend fun stop() {
        driveJob?.cancel()
        withContext(Dispatchers.IO) {
            coordinator.stop()
        }
        audioSink.stop()
        scope.cancel()
    }
}
