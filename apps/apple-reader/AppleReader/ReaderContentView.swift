import SwiftUI
import UniformTypeIdentifiers
import Kit
import Stage
import Actor
import Director

// MARK: - Reader Theme

enum ReaderTheme: String, CaseIterable, Identifiable {
    case dark = "Slate Dark"
    case sepia = "Warm Sepia"
    case light = "Cream Light"
    
    var id: String { self.rawValue }
    
    var backgroundColor: Color {
        switch self {
        case .dark: return Color(red: 0.08, green: 0.09, blue: 0.11)
        case .sepia: return Color(red: 0.96, green: 0.94, blue: 0.89)
        case .light: return Color(red: 0.98, green: 0.98, blue: 0.96)
        }
    }
    
    var textColor: Color {
        switch self {
        case .dark: return Color.white.opacity(0.9)
        case .sepia: return Color(red: 0.22, green: 0.18, blue: 0.12)
        case .light: return Color(red: 0.15, green: 0.15, blue: 0.15)
        }
    }
    
    var highlightColor: Color {
        switch self {
        case .dark: return Color.purple.opacity(0.3)
        case .sepia: return Color.orange.opacity(0.2)
        case .light: return Color.blue.opacity(0.15)
        }
    }
    
    var highlightBorder: Color {
        switch self {
        case .dark: return Color.purple.opacity(0.6)
        case .sepia: return Color.orange.opacity(0.5)
        case .light: return Color.blue.opacity(0.4)
        }
    }
    
    var glassColor: Color {
        switch self {
        case .dark: return Color.white.opacity(0.05)
        case .sepia: return Color.black.opacity(0.03)
        case .light: return Color.black.opacity(0.04)
        }
    }
}

// MARK: - Reader ViewModel

@MainActor
@Observable
final class ReaderViewModel {
    var loadedBook: (any BookDocument)? = nil
    var bookTitle: String = ""
    var chapters: [BookChapter] = []
    
    var selectedChapterIndex: Int? = nil
    var currentChapterSentences: [String] = []
    
    // Playback state
    var isPlaying = false
    var currentSentenceIndex: Int? = nil
    var activeSentenceText: String = ""
    var activeDirective: String = "[V: 0.0 A: 0.0 T: 0.0]"
    var playbackSpeed: Double = 1.0
    
    private var playbackController: (any PlaybackController)? = nil
    private var driveTask: Task<Void, Never>? = nil
    private let segmenter = SentenceSegmenter()
    
    func loadBook(from url: URL) async {
        do {
            let ref = BookReference(fileURL: url)
            let parser = FileBookSourceParser()
            let doc = try await parser.parse(ref)
            
            let title = url.deletingPathExtension().lastPathComponent
            
            var parsedChapters: [BookChapter] = []
            for i in 0..<doc.chapterCount {
                if let ch = try? await doc.chapter(at: i) {
                    parsedChapters.append(ch)
                }
            }
            
            self.loadedBook = doc
            self.bookTitle = title
            self.chapters = parsedChapters
            if !parsedChapters.isEmpty {
                self.selectChapter(at: 0)
            }
        } catch {
            print("Failed to load book: \(error)")
        }
    }

    func selectChapter(at index: Int) {
        guard index >= 0 && index < chapters.count else { return }
        selectedChapterIndex = index
        let rawText = chapters[index].text
        
        // Split into sentences using Apple's SentenceSegmenter
        currentChapterSentences = segmenter.sentences(text: rawText)
        
        stopPlayback()
    }
    
    func startPlayback() {
        guard let doc = loadedBook, let chapterIndex = selectedChapterIndex else { return }
        if playbackController != nil {
            resumePlayback()
            return
        }
        
        let chapter = chapters[chapterIndex]
        let docForPlayback = InMemoryBookDocument(chapters: [chapter.text])
        
        // Use real models if available, otherwise fall back to stubs
        let modelFile = ProductionRunner.mlxModelFile
        let voiceDir = ProductionRunner.mlxDirectory
        
        let director: any Stage.DirectorInference
        let actor: any Stage.VocalActor
        
        if FileManager.default.fileExists(atPath: modelFile.path) {
            actor = VocalActorRegistry.shared.makeActor(for: modelFile, voiceDirectoryURL: voiceDir) ?? StubVocalActor()
        } else {
            actor = StubVocalActor()
        }
        
        let directorModelFile = ProductionRunner.modelsBase.appendingPathComponent("gemma-4-E2B-it.litertlm")
        if FileManager.default.fileExists(atPath: directorModelFile.path) {
            director = DirectorRegistry.shared.makeDirector(for: directorModelFile, narrationMode: .solo) ?? StubDirectorInference()
        } else {
            director = StubDirectorInference()
        }
        
        isPlaying = true
        
        Task {
            let controller = await Stage.StageCoordinator.run(
                document: docForPlayback,
                director: director,
                actor: actor,
                lookahead: 5
            )
            
            self.playbackController = controller
            await actor.updateSpeedMultiplier(playbackSpeed)
            
            driveTask = Task {
                for await event in controller.events {
                    if Task.isCancelled { break }
                    switch event {
                    case .sentenceBegan(let idx):
                        self.currentSentenceIndex = idx
                        if idx >= 0 && idx < self.currentChapterSentences.count {
                            self.activeSentenceText = self.currentChapterSentences[idx]
                        }
                    case .sentenceScheduled(let idx, _):
                        // Visual update
                        self.currentSentenceIndex = idx
                    case .finished:
                        self.isPlaying = false
                        self.currentSentenceIndex = nil
                        self.activeSentenceText = ""
                        self.playbackController = nil
                    case .segmentFailed(let idx, let err):
                        print("Segment \(idx) failed: \(err)")
                    default:
                        break
                    }
                }
            }
        }
    }
    
    func pausePlayback() {
        Task {
            await playbackController?.pause()
            self.isPlaying = false
        }
    }
    
    func resumePlayback() {
        Task {
            await playbackController?.resume()
            self.isPlaying = true
        }
    }
    
    func stopPlayback() {
        driveTask?.cancel()
        driveTask = nil
        let controller = playbackController
        playbackController = nil
        isPlaying = false
        currentSentenceIndex = nil
        activeSentenceText = ""
        
        Task {
            await controller?.stop()
        }
    }
    
    func setSpeed(_ speed: Double) {
        self.playbackSpeed = speed
        if let controller = playbackController {
            // Speed changes can be dynamically sent to the running actor
            // through the registries or direct calls if supported.
            // For now, save speed.
        }
    }
}

// MARK: - ReaderContentView

struct ReaderContentView: View {
    @State private var model = ReaderViewModel()
    @State private var showingFileImporter = false
    @State private var activeTheme = ReaderTheme.dark
    @State private var fontSize: Double = 18.0
    
    var body: some View {
        HStack(spacing: 0) {
            // Left Sidebar: Chapter List (Glassmorphic)
            sidebarView
                .frame(width: 280)
                .background(.ultraThinMaterial)
                .overlay(
                    Rectangle()
                        .stroke(Color.white.opacity(0.1), lineWidth: 1)
                        .alignmentGuide(.leading) { _ in 0 }
                )
            
            // Main Content Area
            VStack(spacing: 0) {
                // Top Bar (Controls / Configs)
                topBarView
                
                // Chapter text reader view
                if model.loadedBook == nil {
                    welcomeSplashView
                } else {
                    readerScrollView
                }
                
                // Bottom Playback Bar (Glassmorphic)
                playbackControlBarView
            }
            .background(activeTheme.backgroundColor)
            .animation(.easeInOut, value: activeTheme)
        }
        .fileImporter(
            isPresented: $showingFileImporter,
            allowedContentTypes: [UTType.epub, UTType.plainText],
            allowsMultipleSelection: false
        ) { result in
            switch result {
            case .success(let urls):
                guard let url = urls.first else { return }
                // Start security scoped accessing if needed
                let access = url.startAccessingSecurityScopedResource()
                Task {
                    await model.loadBook(from: url)
                    if access {
                        url.stopAccessingSecurityScopedResource()
                    }
                }
            case .failure(let error):
                print("Error picking file: \(error)")
            }
        }
    }
    
    // MARK: - Sidebar Component
    
    private var sidebarView: some View {
        VStack(alignment: .leading, spacing: 16) {
            // Library Header
            HStack {
                Text("Library")
                    .font(.system(.title3, design: .rounded))
                    .fontWeight(.bold)
                    .foregroundColor(activeTheme.textColor)
                Spacer()
                Button(action: { showingFileImporter = true }) {
                    Image(systemName: "plus")
                        .font(.body)
                        .foregroundColor(.accentColor)
                        .padding(8)
                        .background(Color.accentColor.opacity(0.15))
                        .clipShape(Circle())
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal)
            .padding(.top, 24)
            
            if model.loadedBook == nil {
                ContentUnavailableView {
                    Label("No Book", systemImage: "book.closed")
                } description: {
                    Text("Import an EPUB or TXT file to begin reading.")
                }
                .foregroundColor(activeTheme.textColor.opacity(0.5))
            } else {
                Text(model.bookTitle)
                    .font(.headline)
                    .lineLimit(2)
                    .padding(.horizontal)
                    .foregroundColor(activeTheme.textColor)
                
                Divider()
                    .background(activeTheme.textColor.opacity(0.2))
                
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 4) {
                        ForEach(model.chapters.indices, id: \.self) { idx in
                            let ch = model.chapters[idx]
                            let isSelected = model.selectedChapterIndex == idx
                            
                            Button(action: { model.selectChapter(at: idx) }) {
                                HStack {
                                    Text("\(idx + 1).")
                                        .font(.system(.body, design: .monospaced))
                                        .fontWeight(isSelected ? .bold : .regular)
                                        .foregroundColor(isSelected ? .accentColor : activeTheme.textColor.opacity(0.7))
                                    
                                    Text(ch.title ?? "Chapter \(idx + 1)")
                                        .font(.body)
                                        .lineLimit(1)
                                        .fontWeight(isSelected ? .bold : .regular)
                                        .foregroundColor(isSelected ? .accentColor : activeTheme.textColor)
                                    Spacer()
                                }
                                .padding(.vertical, 8)
                                .padding(.horizontal, 12)
                                .background(
                                    RoundedRectangle(cornerRadius: 8)
                                        .fill(isSelected ? Color.accentColor.opacity(0.15) : Color.clear)
                                )
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 8)
                }
            }
            Spacer()
        }
    }
    
    // MARK: - Top Config Bar
    
    private var topBarView: some View {
        HStack {
            if model.loadedBook != nil {
                Text(model.chapters[model.selectedChapterIndex ?? 0].title ?? "Reading Pane")
                    .font(.headline)
                    .foregroundColor(activeTheme.textColor)
            }
            Spacer()
            
            // Font size slider
            HStack(spacing: 8) {
                Image(systemName: "textformat.size.smaller")
                    .foregroundColor(activeTheme.textColor.opacity(0.6))
                Slider(value: $fontSize, in: 14...32, step: 1)
                    .frame(width: 120)
                Image(systemName: "textformat.size.larger")
                    .foregroundColor(activeTheme.textColor.opacity(0.6))
            }
            .padding(.trailing, 16)
            
            // Theme picker
            Picker("Theme", selection: $activeTheme) {
                ForEach(ReaderTheme.allCases) { theme in
                    Text(theme.rawValue).tag(theme)
                }
            }
            .pickerStyle(.menu)
            .frame(width: 130)
            .foregroundColor(activeTheme.textColor)
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 12)
        .background(activeTheme.glassColor)
    }
    
    // MARK: - Welcome View
    
    private var welcomeSplashView: some View {
        VStack(spacing: 20) {
            Spacer()
            Image(systemName: "book.pages.fill")
                .font(.system(size: 72))
                .foregroundStyle(
                    .linearGradient(
                        colors: [.purple, .blue],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
                .shadow(color: .purple.opacity(0.4), radius: 10, x: 0, y: 5)
            
            Text("Prosodia Dramatic Reader")
                .font(.system(.title, design: .rounded))
                .fontWeight(.bold)
                .foregroundColor(activeTheme.textColor)
            
            Text("Listen to books perform with local neural models, shifting character voices dynamically for realistic narration.")
                .font(.body)
                .multilineTextAlignment(.center)
                .foregroundColor(activeTheme.textColor.opacity(0.7))
                .frame(maxWidth: 460)
            
            Button(action: { showingFileImporter = true }) {
                Text("Open a Book File")
                    .font(.headline)
                    .foregroundColor(.white)
                    .padding(.vertical, 12)
                    .padding(.horizontal, 24)
                    .background(
                        RoundedRectangle(cornerRadius: 12)
                            .fill(Color.accentColor)
                    )
            }
            .buttonStyle(.plain)
            .padding(.top, 10)
            
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
    
    // MARK: - Main Reader Area
    
    private var readerScrollView: some View {
        ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    ForEach(model.currentChapterSentences.indices, id: \.self) { idx in
                        let sentence = model.currentChapterSentences[idx]
                        let isHighlighted = model.currentSentenceIndex == idx
                        
                        Text(sentence)
                            .font(.custom("Georgia", size: fontSize))
                            .lineSpacing(8)
                            .foregroundColor(activeTheme.textColor)
                            .padding(.vertical, 6)
                            .padding(.horizontal, 12)
                            .background(
                                RoundedRectangle(cornerRadius: 6)
                                    .fill(isHighlighted ? activeTheme.highlightColor : Color.clear)
                            )
                            .overlay(
                                RoundedRectangle(cornerRadius: 6)
                                    .stroke(isHighlighted ? activeTheme.highlightBorder : Color.clear, lineWidth: 1)
                            )
                            .id(idx)
                            .onTapGesture {
                                // Jump playback to sentence
                                model.stopPlayback()
                                model.currentSentenceIndex = idx
                                model.startPlayback()
                            }
                    }
                }
                .padding(.horizontal, 48)
                .padding(.vertical, 32)
            }
            .onChange(of: model.currentSentenceIndex) { _, newIndex in
                if let newIndex = newIndex {
                    withAnimation {
                        proxy.scrollTo(newIndex, anchor: .center)
                    }
                }
            }
        }
    }
    
    // MARK: - Playback Bar Component
    
    private var playbackControlBarView: some View {
        HStack(spacing: 24) {
            // Left: Current reading details
            VStack(alignment: .leading, spacing: 4) {
                if model.loadedBook != nil {
                    Text(model.bookTitle)
                        .font(.caption)
                        .foregroundColor(activeTheme.textColor.opacity(0.6))
                        .lineLimit(1)
                    
                    if !model.activeSentenceText.isEmpty {
                        Text(model.activeSentenceText)
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .foregroundColor(activeTheme.textColor)
                            .lineLimit(1)
                    } else {
                        Text("Ready to read")
                            .font(.subheadline)
                            .foregroundColor(activeTheme.textColor.opacity(0.5))
                    }
                } else {
                    Text("No file loaded")
                        .font(.subheadline)
                        .foregroundColor(activeTheme.textColor.opacity(0.5))
                }
            }
            .frame(width: 250, alignment: .leading)
            
            Spacer()
            
            // Center Controls
            HStack(spacing: 16) {
                // Prev Chapter button
                Button(action: {
                    if let cur = model.selectedChapterIndex, cur > 0 {
                        model.selectChapter(at: cur - 1)
                    }
                }) {
                    Image(systemName: "backward.fill")
                        .font(.title3)
                        .foregroundColor(activeTheme.textColor)
                }
                .disabled(model.loadedBook == nil || model.selectedChapterIndex == 0)
                .buttonStyle(.plain)
                
                // Play / Pause button
                Button(action: {
                    if model.isPlaying {
                        model.pausePlayback()
                    } else {
                        model.startPlayback()
                    }
                }) {
                    Image(systemName: model.isPlaying ? "pause.circle.fill" : "play.circle.fill")
                        .font(.system(size: 40))
                        .foregroundColor(.accentColor)
                }
                .disabled(model.loadedBook == nil)
                .buttonStyle(.plain)
                
                // Stop button
                Button(action: {
                    model.stopPlayback()
                }) {
                    Image(systemName: "stop.fill")
                        .font(.title3)
                        .foregroundColor(activeTheme.textColor)
                }
                .disabled(model.loadedBook == nil)
                .buttonStyle(.plain)
                
                // Next Chapter button
                Button(action: {
                    if let cur = model.selectedChapterIndex, cur < model.chapters.count - 1 {
                        model.selectChapter(at: cur + 1)
                    }
                }) {
                    Image(systemName: "forward.fill")
                        .font(.title3)
                        .foregroundColor(activeTheme.textColor)
                }
                .disabled(model.loadedBook == nil || model.selectedChapterIndex == model.chapters.count - 1)
                .buttonStyle(.plain)
            }
            
            Spacer()
            
            // Right Controls: Speed selection
            HStack(spacing: 8) {
                Image(systemName: "gauge.with.dots.needle.bottom.100percent")
                    .foregroundColor(activeTheme.textColor.opacity(0.6))
                
                Menu {
                    ForEach([0.8, 1.0, 1.2, 1.5, 2.0], id: \.self) { val in
                        Button(action: { model.setSpeed(val) }) {
                            Text(String(format: "%.1fx", val))
                        }
                    }
                } label: {
                    Text(String(format: "%.1fx", model.playbackSpeed))
                        .fontWeight(.semibold)
                        .foregroundColor(activeTheme.textColor)
                }
                .menuStyle(.button)
                .disabled(model.loadedBook == nil)
            }
            .frame(width: 120, alignment: .trailing)
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 16)
        .background(.ultraThinMaterial)
        .overlay(
            Rectangle()
                .stroke(Color.white.opacity(0.1), lineWidth: 1)
                .alignmentGuide(.top) { _ in 0 }
        )
    }
}
