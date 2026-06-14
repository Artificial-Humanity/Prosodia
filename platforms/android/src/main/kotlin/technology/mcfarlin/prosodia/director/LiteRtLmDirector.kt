package technology.mcfarlin.prosodia.director

import kotlinx.coroutines.runBlocking
import technology.mcfarlin.prosodia.stage.NarrationMode

/**
 * A DirectorInference backed by a LiteRT-LM model executed via the Rust core GemmaDirector.
 */
class LiteRtLmDirector(
    modelPath: String,
    narrationMode: NarrationMode = NarrationMode.SOLO
) : uniffi.stage.DirectorInference {

    private val rustDirector = uniffi.director.GemmaDirector(
        modelPath = modelPath,
        contextTokens = 0, // 0 defaults to model/engine config size
        narrationMode = when (narrationMode) {
            NarrationMode.SOLO -> uniffi.director.NarrationMode.SOLO
            NarrationMode.FULL_CAST -> uniffi.director.NarrationMode.FULL_CAST
        }
    )

    fun setNarrationMode(mode: NarrationMode) {
        rustDirector.setNarrationMode(
            mode = when (mode) {
                NarrationMode.SOLO -> uniffi.director.NarrationMode.SOLO
                NarrationMode.FULL_CAST -> uniffi.director.NarrationMode.FULL_CAST
            }
        )
    }

    fun reclaimMemory() {
        rustDirector.reclaimMemory()
    }

    override fun annotate(passage: String): String {
        // Tag passage blocks using the on-device LLM
        return runBlocking {
            rustDirector.tagPassage(passage)
        }
    }
}
