import { mount } from "svelte";
// The interface face ships with the app (no remote fonts, see the CSP).
import "@fontsource-variable/inter/opsz.css";
import "./styles/app.css";
import App from "./App.svelte";

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
