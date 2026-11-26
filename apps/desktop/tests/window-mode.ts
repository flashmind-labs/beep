// The page runs as the command center window's pane, not as the bar. Import it right after
// bar-env.ts, before any module of the bar's page reads the mode.
import { bar } from "./bar-env.ts";

/// The window asks for the phone layout and Reduce Motion; neither applies here.
export const noMedia = (media: string) => ({ matches: false, media, addEventListener() {}, removeEventListener() {} });

Object.assign(bar.dataset, { mode: "window" });
Object.assign(globalThis, { matchMedia: noMedia });
