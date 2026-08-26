import fs from "node:fs";

const packageJson = JSON.parse(fs.readFileSync(new URL("../package.json", import.meta.url), "utf8"));
const tauriConfig = JSON.parse(fs.readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
const cargoToml = fs.readFileSync(new URL("../src-tauri/Cargo.toml", import.meta.url), "utf8");
const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

const versions = {
  "package.json": packageJson.version,
  "src-tauri/tauri.conf.json": tauriConfig.version,
  "src-tauri/Cargo.toml": cargoVersion,
};
const expected = packageJson.version;
const mismatches = Object.entries(versions).filter(([, version]) => version !== expected);
if (mismatches.length) {
  for (const [file, version] of mismatches) {
    console.error(`${file}: expected ${expected}, found ${version ?? "missing"}`);
  }
  process.exit(1);
}
console.log(`Modelay version ${expected} is consistent across package, Tauri, and Cargo manifests.`);
