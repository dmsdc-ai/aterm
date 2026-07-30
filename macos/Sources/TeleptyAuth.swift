import Foundation

/// Credential for the telepty daemon's HTTP + WebSocket API.
///
/// The daemon writes `authToken` into ~/.telepty/config.json. That file — not an
/// env var — is the path that must work unaided: a GUI launched from Finder
/// inherits no shell environment, so an env-only token passes every terminal
/// test and fails every real user. `TELEPTY_AUTH_TOKEN` is honoured as an
/// override when it happens to be set.
enum TeleptyAuth {
  static let header = "x-telepty-token"

  /// Resolved per request, not cached at startup: on a fresh install the daemon
  /// writes the config moments after aterm launches, so a startup read would
  /// cache a permanent empty. Reading a small local file per request also picks
  /// up token rotation for free.
  ///
  /// Returns nil when the config is absent or malformed — the caller then sends
  /// no header and degrades to the "daemon unavailable" path it already has.
  /// Never logged.
  static func token() -> String? {
    if let env = ProcessInfo.processInfo.environment["TELEPTY_AUTH_TOKEN"], !env.isEmpty {
      return env
    }
    // $HOME first, passwd entry as fallback — the same order telepty itself
    // uses to pick where it writes this file, and the same as the Rust bridge.
    let home =
      ProcessInfo.processInfo.environment["HOME"]
      ?? FileManager.default.homeDirectoryForCurrentUser.path
    let path = URL(fileURLWithPath: home).appendingPathComponent(".telepty/config.json")
    guard let data = try? Data(contentsOf: path),
      let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
      let token = json["authToken"] as? String, !token.isEmpty
    else { return nil }
    return token
  }

  /// Attach the credential to a request. No-op when unresolved.
  static func authorize(_ request: inout URLRequest) {
    if let token = token() {
      request.setValue(token, forHTTPHeaderField: header)
    }
  }
}
