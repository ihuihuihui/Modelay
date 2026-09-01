export type SessionScope = "recent5" | "all" | "single";

export function resolveSessionScope(isCurrentChannel: boolean, requested: SessionScope): SessionScope {
  // A cross-channel switch must migrate the selected tasks so Codex opens
  // them with the newly activated provider instead of the exhausted one.
  return requested;
}
