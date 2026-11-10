import "../ui/window-defaults.ts";
import "../ui/zoom.ts";
import { followSkin } from "../ui/skin.ts";
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { Orb } from "./orb-view.tsx";

followSkin();
createRoot(document.body).render(createElement(Orb));
