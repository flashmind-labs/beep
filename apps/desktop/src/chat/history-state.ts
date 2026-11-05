// Which turns of a long chat are drawn: the newest few, with the rest
// drawn as the reader scrolls up.

/// How many prompts a chat draws when it opens, and each time it draws earlier ones.
export const HISTORY_CHUNK = 40;

/// Where the last `count` prompts begin in a chat's messages, by role: the
/// index to draw from. 0 when the chat has no more prompts than that.
export function windowStart(roles: readonly string[], count: number) {
  let seen = 0;
  for (let i = roles.length - 1; i >= 0; i--) {
    if (roles[i] !== "user" && roles[i] !== "scheduled") continue;
    if (++seen === count) return i;
  }
  return 0;
}
