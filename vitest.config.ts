import { defineConfig, mergeConfig } from "vitest/config";
import { svelteTesting } from "@testing-library/svelte/vite";
import viteConfig from "./vite.config.ts";

// The interface's unit tests (`npm test`): the app's own Vite config, so
// runes in `.svelte.ts` files compile as they do in the app, plus a small
// DOM for the components. `svelteTesting` resolves Svelte's browser build
// and unmounts each component after its test.
export default mergeConfig(
  viteConfig,
  defineConfig({
    plugins: [svelteTesting()],
    test: {
      environment: "happy-dom",
      include: ["src/**/*.test.ts"],
    },
  }),
);
