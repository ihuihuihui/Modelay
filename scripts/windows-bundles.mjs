import { readFile } from "node:fs/promises";

const { version } = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
const prerelease = version.split("-", 2)[1];
const wixCompatible = !prerelease || /^\d+$/.test(prerelease);

process.stdout.write(wixCompatible ? "nsis,msi" : "nsis");
