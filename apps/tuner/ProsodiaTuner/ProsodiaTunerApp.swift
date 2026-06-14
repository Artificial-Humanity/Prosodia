//
//  ProsodiaTunerApp.swift
//  ProsodiaTuner
//

import SwiftUI
import Misaki
import ActorEspeak
import Kit
import Actor
import Director

@main
struct ProsodiaTunerApp: App {
    init() {
        registerProsodiaDirectors()
        registerProsodiaActors()
        if let espeak = try? NativeEspeakG2PProcessor() {
            G2P.defaultFallback = { word, british in
                let lang = british ? "en-gb" : "en-us"
                let res = try? espeak.phonemize(word, langCode: lang)
                return res?.phonemes
            }
        }
    }

    var body: some Scene {
        WindowGroup {
            TunerContentView()
        }
    }
}
