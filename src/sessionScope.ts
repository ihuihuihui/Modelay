export type SessionScope = "none" | "recent5" | "all" | "single";
export type CrossChannelMode = "smart" | "switchOnly" | "migrate";

export function resolveSessionScope(
  isCurrentChannel: boolean,
  requested: SessionScope,
  crossChannelMode: CrossChannelMode,
): SessionScope {
  return isCurrentChannel || crossChannelMode === "migrate" ? requested : "none";
}

export function switchRequiresThread(
  isCurrentChannel: boolean,
  requested: SessionScope,
  crossChannelMode: CrossChannelMode,
): boolean {
  return isCurrentChannel
    ? requested === "single"
    : crossChannelMode === "smart" ||
        (crossChannelMode === "migrate" && requested === "single");
}
