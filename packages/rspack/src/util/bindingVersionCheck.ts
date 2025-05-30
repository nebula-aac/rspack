import { readdirSync } from "node:fs";
import path from "node:path";

const NodePlatformArchToAbi: Record<
	string,
	Record<string, string | Record<string, string>>
> = {
	android: {
		arm64: "",
		arm: "eabi"
	},
	win32: {
		x64: "msvc",
		ia32: "msvc",
		arm64: "msvc"
	},
	darwin: {
		x64: "",
		arm64: ""
	},
	freebsd: {
		x64: ""
	},
	linux: {
		x64: {
			musl: "musl",
			gnu: "gnu"
		},
		arm64: {
			musl: "musl",
			gnu: "gnu"
		},
		arm: "gnueabihf"
	}
};

function isMusl() {
	// @ts-expect-error getReport returns an object containing header object
	const { glibcVersionRuntime } = process.report.getReport().header;
	return !glibcVersionRuntime;
}

const BINDING_VERSION = require("@rspack/binding/package.json").version;
const CORE_VERSION = RSPACK_VERSION;

const getAddonPlatformArchAbi = () => {
	const { platform, arch } = process;
	let binding = "";
	binding += platform;

	const abi = NodePlatformArchToAbi[platform][arch];
	if (abi === undefined) return new Error(`unsupported cpu arch: ${arch}`);
	binding += `-${arch}`;

	if (typeof abi === "string") {
		binding += abi.length ? `-${abi}` : "";
	} else if (typeof abi === "object") {
		binding += `-${abi[isMusl() ? "musl" : "gnu"]}`;
	} else {
		return new Error(`unsupported abi: ${abi}`);
	}

	return binding;
};

/**
 * Error: version checked with error
 * null: version checked without any error
 * undefined: version to be checked
 */
let result: Error | undefined | null;

/**
 * Check if these version matches:
 * `@rspack/core`, `@rspack/binding`, `@rspack/binding-<platform>-<arch>-<abi>`
 */
export const checkVersion = () => {
	if (result !== undefined) {
		return result;
	}

	const platformArchAbi = getAddonPlatformArchAbi();
	if (platformArchAbi instanceof Error) {
		return (result = platformArchAbi);
	}

	let ADDON_VERSION: string;
	try {
		const BINDING_PKG_DIR = path.dirname(
			require.resolve("@rspack/binding/package.json")
		);

		const isLocal = readdirSync(BINDING_PKG_DIR).some(
			item =>
				item === `rspack.${platformArchAbi}.node` || "rspack.wasm32-wasi.wasm"
		);

		if (isLocal) {
			// Treat addon version the same as binding version if running locally
			ADDON_VERSION = BINDING_VERSION;
		} else {
			// Fetch addon package if installed from remote
			try {
				ADDON_VERSION = require(
					require.resolve(`@rspack/binding-${platformArchAbi}/package.json`, {
						paths: [BINDING_PKG_DIR]
					})
				).version;
			} catch {
				// Wasm fallback
				ADDON_VERSION = require(
					require.resolve("@rspack/binding-wasm32-wasi/package.json", {
						paths: [BINDING_PKG_DIR]
					})
				).version;
			}
		}
	} catch (error: any) {
		if (error instanceof Error) {
			return (result = new Error(
				`${error.toString()}. Maybe you forget to install the correct addon package ${`@rspack/binding-${platformArchAbi}`} or forget to build binding locally?`
			));
		}
		return (result = new Error(error));
	}

	const isMatch = [CORE_VERSION, BINDING_VERSION, ADDON_VERSION].every(
		(v, _, arr) => v === arr[0]
	);

	if (!isMatch) {
		return (result = new Error(
			`Unmatched version @rspack/core@${CORE_VERSION}, @rspack/binding@${BINDING_VERSION}, @rspack/binding-${platformArchAbi}@${ADDON_VERSION}.\nRspack requires these versions to be the same or you may have installed the wrong version. Otherwise, Rspack may not work properly.`
		));
	}

	return (result = null);
};
