//
//  ProsodiaTunerApp.swift
//  ProsodiaTuner
//

import SwiftUI
import Kit
import Actor
import Director

@main
struct ProsodiaTunerApp: App {
    init() {
        registerProsodiaDirectors()
        registerProsodiaActors()
    }

    var body: some Scene {
        WindowGroup {
            TunerContentView()
        }
    }
}
