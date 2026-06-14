package technology.mcfarlin.prosodia.stage

import kotlinx.coroutines.flow.Flow
import java.io.File
import java.util.UUID

// MARK: - Shared Value Types

/**
 * A word-level timestamp indicating the relative start/end seconds and character offset within a sentence.
 */
data class TokenTimestamp(
    val text: String,
    val characterOffset: Int,
    val startSeconds: Double,
    val endSeconds: Double
)

// MARK: - Playback Control Plane

/**
 * Lifecycle events emitted by an active PlaybackController.
 */
sealed class PlaybackEvent {
    data class SentenceBegan(val index: Int) : PlaybackEvent()
    data class SentenceScheduled(val index: Int, val timestamps: List<TokenTimestamp>) : PlaybackEvent()
    data class PlaybackProgress(val index: Int, val characterOffset: Int) : PlaybackEvent()
    object Finished : PlaybackEvent()
    data class SegmentFailed(val index: Int, val error: Throwable) : PlaybackEvent()
}

/**
 * A handle that lets callers pause, resume, and stop an active render session, and observe lifecycle events.
 */
interface PlaybackController {
    suspend fun pause()
    suspend fun resume()
    suspend fun stop()
    val events: Flow<PlaybackEvent>
}

// MARK: - Service Protocols

enum class NarrationMode {
    SOLO,
    FULL_CAST
}

data class BookReference(
    val id: UUID = UUID.randomUUID(),
    val filePath: String
)

data class BookChapter(
    val spineIndex: Int,
    val title: String?,
    val text: String
)

interface BookDocument {
    val chapterCount: Int
    suspend fun chapter(atIndex: Int): BookChapter
}

// MARK: - Stub Implementations

class InMemoryBookDocument(private val chapters: List<BookChapter>) : BookDocument {
    constructor(chapterTexts: List<String>) : this(
        chapterTexts.mapIndexed { index, text -> BookChapter(index, null, text) }
    )

    override val chapterCount: Int = chapters.size

    override suspend fun chapter(atIndex: Int): BookChapter {
        if (atIndex < 0 || atIndex >= chapterCount) {
            throw IndexOutOfBoundsException("Chapter index $atIndex out of range 0..$chapterCount")
        }
        return chapters[atIndex]
    }
}

class PlainTextBookParser {
    fun parse(reference: BookReference): BookDocument {
        val file = File(reference.filePath)
        if (!file.exists() || !file.canRead()) {
            throw IllegalArgumentException("Cannot read book reference file: ${reference.filePath}")
        }
        val raw = file.readText(Charsets.UTF_8)
        val normalized = raw.replace("\r\n", "\n")
        
        // Split by form-feed character U+000C
        val sections = normalized.split("\u000C")
            .map { it.trim() }
            .filter { it.isNotEmpty() }

        if (sections.isEmpty()) {
            throw IllegalArgumentException("Document contains no readable text")
        }
        return InMemoryBookDocument(sections)
    }
}
