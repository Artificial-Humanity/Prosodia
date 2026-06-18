package technology.mcfarlin.prosodia.actor

import uniffi.actor.ActorEngineOutput
import uniffi.actor.PipelineOutput
import uniffi.actor.ProsodiaSpeechEngine
import uniffi.actor.StyleVector

/**
 * Bridges the UniFFI callback interface for ProsodiaSpeechEngine to the LiteRtActorEngine backend.
 */
class KotlinSpeechEngine(
    private val backend: uniffi.actor.LiteRtActorEngine
) : ProsodiaSpeechEngine {
    override fun synthesize(input: PipelineOutput): ActorEngineOutput {
        throw UnsupportedOperationException("synthesize is deprecated, use forward instead")
    }

    override fun forward(
        phonemeIds: List<Int>,
        style: StyleVector,
        speed: Float,
        vat: List<Float>?,
        durationScales: List<Float>?,
        f0Bias: List<Float>?
    ): ActorEngineOutput {
        return backend.forward(
            phonemeIds = phonemeIds,
            style = style,
            speed = speed,
            vat = vat,
            durationScales = durationScales,
            f0Bias = f0Bias
        )
    }

    override fun reclaimMemory() {
        backend.reclaimMemory()
    }
}

/**
 * A VocalActor implementation wrapped around ProsodiaActorEngine driving StyleTTS2 via LiteRT on Android.
 */
class DiskVoiceAssetProvider(private val baseDirectory: String) : uniffi.actor.VoiceAssetProvider {
    override fun loadVoiceBytes(voiceName: String): ByteArray? {
        val file = java.io.File(baseDirectory, voiceName)
        return if (file.exists() && file.isFile) {
            file.readBytes()
        } else {
            null
        }
    }
}

/**
 * A VocalActor implementation wrapped around ProsodiaActorEngine driving StyleTTS2 via LiteRT on Android.
 */
class LiteRtVocalActor(
    modelPath: String,
    configPath: String,
    voiceDirectoryPath: String
) : uniffi.stage.VocalActor {

    private val rustEngine: uniffi.actor.ProsodiaActorEngine

    init {
        val provider = DiskVoiceAssetProvider(voiceDirectoryPath)
        val voiceLoader = uniffi.actor.VoiceLoader(provider)
        val g2pObject = uniffi.actor.ProsodiaSpeech()
        val g2p = object : uniffi.actor.ProsodiaG2pProcessor {
            override fun process(text: String): List<uniffi.actor.MToken> {
                return g2pObject.process(text)
            }
        }
        val configJson = java.io.File(configPath).readText(Charsets.UTF_8)

        val pipeline = uniffi.actor.ProsodiaActorPipeline(
            g2p = g2p,
            voiceLoader = voiceLoader,
            configJson = configJson,
            sampleRate = uniffi.stage.getSampleRate(),
            langCode = "en-us"
        )
        val backend = uniffi.actor.LiteRtActorEngine(modelPath)
        val speechEngine = KotlinSpeechEngine(backend)

        rustEngine = uniffi.actor.ProsodiaActorEngine(pipeline, speechEngine)
    }

    fun reclaimMemory() {
        rustEngine.reclaimMemory()
    }

    override fun render(payload: String): List<Float> {
        val decoded = uniffi.stage.decodeSpans(payload) ?: return emptyList()
        val totalAudio = mutableListOf<Float>()

        for (span in decoded.spans) {
            val kitEmotion = uniffi.stage.EmotionVector(
                valence = span.emotion.valence,
                arousal = span.emotion.arousal,
                tension = span.emotion.tension
            )

            val acoustics = span.acoustics
            val kitAcoustics = if (acoustics != null) {
                val cp = acoustics.castingProfile
                val kitCastingProfile = if (cp != null) {
                    uniffi.stage.CastingProfile(
                        ageProfile = cp.ageProfile,
                        masculinity = cp.masculinity,
                        strainOrRasp = cp.strainOrRasp
                    )
                } else {
                    null
                }

                uniffi.stage.ProsodyAcoustics(
                    speedMultiplier = acoustics.speedMultiplier,
                    speedBias = acoustics.speedBias,
                    gainMultiplier = acoustics.gainMultiplier,
                    gainBias = acoustics.gainBias,
                    castingProfile = kitCastingProfile,
                    speakerLock = acoustics.speakerLock,
                    pauseMultiplier = acoustics.pauseMultiplier,
                    pronunciationOverride = acoustics.pronunciationOverride,
                    pitch = acoustics.pitch,
                    tokenDurationScales = acoustics.tokenDurationScales,
                    tokenF0Biases = acoustics.tokenF0Biases
                )
            } else {
                null
            }

            val kitSpan = uniffi.stage.ProsodySpan(
                text = span.text,
                emotion = kitEmotion,
                leadingPause = span.leadingPause,
                acoustics = kitAcoustics
            )

            try {
                val output = rustEngine.processAndSynthesize(kitSpan)
                totalAudio.addAll(output.audio)
            } catch (e: Exception) {
                System.err.println("Warning: failed to process and synthesize span: ${e.message}")
            }
        }

        return totalAudio
    }
}
