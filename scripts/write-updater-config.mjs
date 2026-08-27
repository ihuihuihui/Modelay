import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const endpoint = process.env.MODELAY_UPDATE_ENDPOINT?.trim() || process.argv[2]?.trim();
if (!endpoint) {
  throw new Error("MODELAY_UPDATE_ENDPOINT is required.");
}

const parsed = new URL(endpoint);
if (parsed.protocol !== "https:") {
  throw new Error("The production updater endpoint must use HTTPS.");
}

const output = resolve("src-tauri/tauri.release.generated.json");
const config = {
  bundle: { createUpdaterArtifacts: true },
  plugins: { updater: { endpoints: [parsed.toString()] } },
};

await writeFile(output, `${JSON.stringify(config, null, 2)}\n`, { mode: 0o600 });
console.log(`Prepared signed updater config for ${parsed.origin}`);
