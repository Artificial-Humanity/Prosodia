//
//  SamplePassageStore.swift
//  AppleReader
//

import Foundation
import Observation

/// Manages harness sample passages from `SamplePassages.txt`.
///
/// A harness build phase creates `SamplePassages.txt` from
/// `SamplePassages.txt.example` when the editable file is missing. The editable
/// file is intentionally gitignored so local audition text can change freely.
@MainActor
@Observable
final class SamplePassageStore {
    static let shared = SamplePassageStore()
    
    var passages: [String] = []
    
    private var fileURL: URL {
        let sourceDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        return sourceDir.appendingPathComponent("SamplePassages.txt")
    }
    
    private init() {
        load()
    }
    
    func load() {
        guard let contents = try? String(contentsOf: fileURL, encoding: .utf8) else {
            self.passages = Self.defaultPassages
            return
        }
        let loaded = contents
            .components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty && !$0.hasPrefix("#") }
        
        if loaded.isEmpty {
            self.passages = Self.defaultPassages
        } else {
            self.passages = loaded
        }
    }
    
    private static let defaultPassages = [
        "The morning light spilled across the quiet kitchen table.",
        "Suddenly, a shadow moved at the edge of the room.",
        "She loved him, softly and without condition.",
        "The function returns a vector of normalized data points.",
        "He had died alone, and the old house remembered him.",
        "We won! We actually won the championship!",
        "I will slay thee, vile troll!",
        "He drove his Corvette Z06 down the stretch of highway like a bat out of hell."
    ]
    
    func add(_ passage: String) {
        let trimmed = passage.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        passages.append(trimmed)
        save()
    }
    
    func remove(at offsets: IndexSet) {
        for index in offsets.reversed() {
            passages.remove(at: index)
        }
        save()
    }
    
    func delete(_ passage: String) {
        passages.removeAll { $0 == passage }
        save()
    }
    
    private func save() {
        let contents = passages.joined(separator: "\n") + "\n"
        try? contents.write(to: fileURL, atomically: true, encoding: .utf8)
    }
}
