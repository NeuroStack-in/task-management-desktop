import { render } from "preact";
import { App } from "./app";
import { applyThemeFromSettings } from "./lib/useTheme";
import "./index.css";

// Apply the saved theme before the first paint so there's no flash of the wrong palette.
applyThemeFromSettings();

render(<App />, document.getElementById("app")!);
