export type SessionScope = "recent5" | "all" | "single";

export function resolveSessionScope(isCurrentChannel: boolean, requested: SessionScope): SessionScope {
  return isCurrentChannel ? requested : "all";
}
