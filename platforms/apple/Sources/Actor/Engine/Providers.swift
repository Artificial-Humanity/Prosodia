import Foundation
import Stage

/// Entry point to register all compiled vocal actor backends with the registry.
///
/// The only supported actor backend is StyleTTS2 via LiteRT (``LiteRtActorEngine``).
///
/// - TODO: Register a LiteRT-backed ``VocalActorProvider`` here once the streaming
///   ``VocalActor`` wrapper around ``LiteRtActorEngine`` is implemented. The MLX and
///   GGUF providers were removed as part of the move to a LiteRT-only actor.
public func registerProsodiaActors() {
    // No-op until the LiteRT ``VocalActor`` wrapper/provider lands.
}
