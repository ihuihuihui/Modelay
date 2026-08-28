export type SessionScope = "recent5" | "all" | "single" | "none";

export function resolveSessionScope(isCurrentChannel: boolean, requested: SessionScope): SessionScope {
  return isCurrentChannel ? requested : "none";
}
