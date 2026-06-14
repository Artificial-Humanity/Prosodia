import Foundation

/// A lightweight handle to a book on disk. The engine resolves the concrete
/// parser (txt / pdf / epub / mobi) from `fileURL` at ingestion time.
public struct BookReference: Sendable, Equatable {
    /// The unique identifier of the book.
    public let id: UUID
    /// The local file URL pointing to the book on disk.
    public let fileURL: URL

    /// Initializes a new BookReference.
    ///
    /// - Parameters:
    ///   - id: The unique identifier (defaults to a random UUID).
    ///   - fileURL: The local URL pointing to the file.
    public init(id: UUID = UUID(), fileURL: URL) {
        self.id = id
        self.fileURL = fileURL
    }
}

// MARK: - Ingestion Model (Stage 1)
//
// Ingestion turns a `BookReference` (a file on disk) into a `BookDocument`: a
// random-access collection of clean, tag-stripped chapters indexed by spine
// position. Random access (rather than a one-shot stream) is what lets the
// engine run the spec's Three-Chapter JIT pipeline (N-1 cached, N active, N+1
// lookahead) and resume from a persisted `spineIndex` + `characterOffset`.

/// A single clean-text chapter, indexed by its position in the manifest spine.
public struct BookChapter: Sendable, Equatable {
    /// Position in the manifest spine — the unit `PlaybackBookmark.spineIndex` tracks.
    public let spineIndex: Int
    /// Structural title, when the format exposes one (e.g. EPUB nav, PDF page label).
    public let title: String?
    /// Clean plain text, stripped of any structural / layout markup.
    public let text: String

    public init(spineIndex: Int, title: String? = nil, text: String) {
        self.spineIndex = spineIndex
        self.title = title
        self.text = text
    }
}

/// A parsed book exposed as lazily-loaded chapters. Conformers keep only what
/// they must resident (e.g. a PDF document handle), reading each chapter on demand.
public protocol BookDocument: Sendable {
    /// Number of chapters in the spine.
    var chapterCount: Int { get }
    /// The chapter at a spine index. Throws ``ParserError/chapterIndexOutOfRange(_:)``
    /// when `index` is outside `0..<chapterCount`.
    func chapter(at index: Int) async throws -> BookChapter
}

public extension BookDocument {
    /// Bridges a document into the `AsyncStream<String>` the Director consumes,
    /// yielding chapter text in spine order. The stream finishes early if a
    /// chapter cannot be read.
    func chapterStream() -> AsyncStream<String> {
        AsyncStream { continuation in
            Task {
                for index in 0..<chapterCount {
                    guard let chapter = try? await chapter(at: index) else { break }
                    continuation.yield(chapter.text)
                }
                continuation.finish()
            }
        }
    }
}

/// Turns a `BookReference` into a `BookDocument`. The concrete parser is chosen
/// from the file's format (see ``FileBookSourceParser``).
public protocol BookParsing: Sendable {
    func parse(_ reference: BookReference) async throws -> any BookDocument
}

/// Errors surfaced while parsing a book source.
public enum ParserError: Error, Sendable, Equatable {
    /// The file's format has no parser yet (e.g. EPUB/MOBI today, or KFX ever).
    case unsupportedFormat(String)
    /// The file could not be read or decoded.
    case unreadableFile(URL)
    /// The file parsed but contained no readable text.
    case emptyDocument
    /// A chapter was requested outside `0..<chapterCount`.
    case chapterIndexOutOfRange(Int)
}

// MARK: - Format Detection

/// The supported (and not-yet-supported) book formats, derived from a file extension.
public enum BookFormat: Sendable, Equatable {
    case plainText
    case pdf
    case epub
    case mobi
    case azw3
    case unknown

    public init(url: URL) {
        switch url.pathExtension.lowercased() {
        case "txt", "text", "md", "markdown":
            self = .plainText
        case "pdf":
            self = .pdf
        case "epub":
            self = .epub
        case "mobi", "prc":
            self = .mobi
        case "azw", "azw3":
            self = .azw3
        default:
            self = .unknown
        }
    }
}

// MARK: - Dispatching Parser

/// The entry point for ingestion: inspects the file format and delegates to the
/// matching concrete parser. Formats without a parser yet throw
/// ``ParserError/unsupportedFormat(_:)``.
public struct FileBookSourceParser: BookParsing {
    public init() {}

    public func parse(_ reference: BookReference) async throws -> any BookDocument {
        switch BookFormat(url: reference.fileURL) {
        case .plainText:
            return try await PlainTextBookParser().parse(reference)
        case .pdf:
            #if canImport(PDFKit)
            return try await PDFBookParser().parse(reference)
            #else
            throw ParserError.unsupportedFormat("pdf (PDFKit is unavailable on this platform)")
            #endif
        case .epub:
            return try await EPUBBookParser().parse(reference)
        case .mobi, .azw3, .unknown:
            throw ParserError.unsupportedFormat(reference.fileURL.pathExtension.isEmpty
                ? "(no extension)" : reference.fileURL.pathExtension)
        }
    }
}

// MARK: - In-Memory Document

/// A document backed by chapters already in memory. Used by the plain-text
/// parser, by tests, and to drive the pipeline without touching disk.
public struct InMemoryBookDocument: BookDocument {
    private let chapters: [BookChapter]

    public init(chapters: [String]) {
        self.chapters = chapters.enumerated().map { index, text in
            BookChapter(spineIndex: index, text: text)
        }
    }

    public init(chapters: [BookChapter]) {
        self.chapters = chapters
    }

    public var chapterCount: Int { chapters.count }

    public func chapter(at index: Int) async throws -> BookChapter {
        guard chapters.indices.contains(index) else {
            throw ParserError.chapterIndexOutOfRange(index)
        }
        return chapters[index]
    }
}

// MARK: - Plain Text (.txt)

/// Plain-text ingestion via a memory-mapped UTF-8 read. Sections separated by a
/// form-feed (`U+000C`, the conventional text page/section break) become
/// chapters; a file with no form feeds is a single chapter.
public struct PlainTextBookParser: BookParsing {
    public init() {}

    public func parse(_ reference: BookReference) async throws -> any BookDocument {
        let url = reference.fileURL
        guard let data = try? Data(contentsOf: url, options: .mappedIfSafe),
              let raw = String(data: data, encoding: .utf8) else {
            throw ParserError.unreadableFile(url)
        }

        let normalized = raw.replacingOccurrences(of: "\r\n", with: "\n")
        let sections = normalized
            .components(separatedBy: "\u{0C}")
            .map { $0.trimmingCharacters(in: CharacterSet.whitespacesAndNewlines) }
            .filter { !$0.isEmpty }

        guard !sections.isEmpty else { throw ParserError.emptyDocument }
        return InMemoryBookDocument(chapters: sections)
    }
}

// MARK: - PDF (.pdf)

#if canImport(PDFKit)
import PDFKit

/// PDF ingestion via Apple's `PDFKit`, extracting text page-by-page. Each page
/// is a chapter, read lazily so large PDFs stay off the heap until needed.
public struct PDFBookParser: BookParsing {
    public init() {}

    public func parse(_ reference: BookReference) async throws -> any BookDocument {
        try PDFBookDocument(url: reference.fileURL)
    }
}

/// Holds the `PDFDocument` behind actor isolation (PDFKit types aren't `Sendable`)
/// and extracts page text on demand.
public actor PDFBookDocument: BookDocument {
    private let document: PDFDocument
    public nonisolated let chapterCount: Int

    public init(url: URL) throws {
        guard let document = PDFDocument(url: url) else {
            throw ParserError.unreadableFile(url)
        }
        guard document.pageCount > 0 else {
            throw ParserError.emptyDocument
        }
        self.document = document
        self.chapterCount = document.pageCount
    }

    public func chapter(at index: Int) async throws -> BookChapter {
        guard index >= 0, index < chapterCount, let page = document.page(at: index) else {
            throw ParserError.chapterIndexOutOfRange(index)
        }
        return BookChapter(spineIndex: index, title: page.label, text: page.string ?? "")
    }
}
#endif

// MARK: - EPUB (.epub)

/// EPUB ingestion using the Rust-based FolioParser FFI library.
public struct EPUBBookParser: BookParsing {
    public init() {}

    public func parse(_ reference: BookReference) async throws -> any BookDocument {
        do {
            let path = reference.fileURL.path
            let chapters = try parseEpub(epubPath: path)
            let bookChapters = chapters.map { ch in
                BookChapter(spineIndex: Int(ch.spineIndex), title: ch.title, text: ch.text)
            }
            guard !bookChapters.isEmpty else {
                throw ParserError.emptyDocument
            }
            return InMemoryBookDocument(chapters: bookChapters)
        } catch {
            throw ParserError.unreadableFile(reference.fileURL)
        }
    }
}
