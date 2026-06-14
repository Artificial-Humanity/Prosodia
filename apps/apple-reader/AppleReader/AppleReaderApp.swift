//
//  AppleReaderApp.swift
//  AppleReader
//

import SwiftUI
import Kit
import Actor
import Director

@main
struct AppleReaderApp: App {
    init() {
        registerProsodiaDirectors()
        registerProsodiaActors()
    }

    var body: some Scene {
        WindowGroup {
            ReaderContentView()
        }
    }
}
