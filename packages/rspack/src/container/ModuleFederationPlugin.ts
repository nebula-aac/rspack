import type { Compiler } from "../Compiler";
import type { ExternalsType } from "../config";
import { getExternalsTypeSchema } from "../schema/config";
import { isValidate } from "../schema/validate";
import type { ModuleFederationPluginV1Options } from "./ModuleFederationPluginV1";
import { ModuleFederationRuntimePlugin } from "./ModuleFederationRuntimePlugin";
import { parseOptions } from "./options";

export interface ModuleFederationPluginOptions
	extends Omit<ModuleFederationPluginV1Options, "enhanced"> {
	runtimePlugins?: RuntimePlugins;
	implementation?: string;
	shareStrategy?: "version-first" | "loaded-first";
}
export type RuntimePlugins = string[];

export class ModuleFederationPlugin {
	constructor(private _options: ModuleFederationPluginOptions) {}

	apply(compiler: Compiler) {
		const { webpack } = compiler;
		const paths = getPaths(this._options);
		compiler.options.resolve.alias = {
			"@module-federation/runtime-tools": paths.runtimeTools,
			"@module-federation/runtime": paths.runtime,
			...compiler.options.resolve.alias
		};

		// Generate the runtime entry content
		const entryRuntime = getDefaultEntryRuntime(paths, this._options, compiler);

		// Pass only the entry runtime to the Rust-side plugin
		new ModuleFederationRuntimePlugin({
			entryRuntime
		}).apply(compiler);

		new webpack.container.ModuleFederationPluginV1({
			...this._options,
			enhanced: true
		}).apply(compiler);
	}
}

interface RuntimePaths {
	runtimeTools: string;
	bundlerRuntime: string;
	runtime: string;
}

interface RemoteInfo {
	alias: string;
	name?: string;
	entry?: string;
	externalType: ExternalsType;
	shareScope: string;
}

type RemoteInfos = Record<string, RemoteInfo[]>;

function getRemoteInfos(options: ModuleFederationPluginOptions): RemoteInfos {
	if (!options.remotes) {
		return {};
	}

	function extractUrlAndGlobal(urlAndGlobal: string) {
		const index = urlAndGlobal.indexOf("@");
		if (index <= 0 || index === urlAndGlobal.length - 1) {
			return null;
		}
		return [
			urlAndGlobal.substring(index + 1),
			urlAndGlobal.substring(0, index)
		] as const;
	}

	function getExternalTypeFromExternal(external: string) {
		if (/^[a-z0-9-]+ /.test(external)) {
			const idx = external.indexOf(" ");
			return [
				external.slice(0, idx) as ExternalsType,
				external.slice(idx + 1)
			] as const;
		}
		return null;
	}

	function getExternal(external: string) {
		const result = getExternalTypeFromExternal(external);
		if (result === null) {
			return [remoteType, external] as const;
		}
		return result;
	}

	const remoteType =
		options.remoteType ||
		(options.library && isValidate(options.library.type, getExternalsTypeSchema)
			? (options.library.type as ExternalsType)
			: "script");
	const remotes = parseOptions(
		options.remotes,
		item => ({
			external: Array.isArray(item) ? item : [item],
			shareScope: options.shareScope || "default"
		}),
		item => ({
			external: Array.isArray(item.external) ? item.external : [item.external],
			shareScope: item.shareScope || options.shareScope || "default"
		})
	);

	const remoteInfos: Record<string, RemoteInfo[]> = {};
	for (const [key, config] of remotes) {
		for (const external of config.external) {
			const [externalType, externalRequest] = getExternal(external);
			remoteInfos[key] ??= [];
			if (externalType === "script") {
				const [url, global] = extractUrlAndGlobal(externalRequest)!;
				remoteInfos[key].push({
					alias: key,
					name: global,
					entry: url,
					externalType,
					shareScope: config.shareScope
				});
			} else {
				remoteInfos[key].push({
					alias: key,
					name: undefined,
					entry: undefined,
					externalType,
					shareScope: config.shareScope
				});
			}
		}
	}
	return remoteInfos;
}

function getRuntimePlugins(options: ModuleFederationPluginOptions) {
	return options.runtimePlugins ?? [];
}

function getPaths(options: ModuleFederationPluginOptions): RuntimePaths {
	const runtimeToolsPath =
		options.implementation ??
		require.resolve("@module-federation/runtime-tools");
	const bundlerRuntimePath = require.resolve(
		"@module-federation/webpack-bundler-runtime",
		{ paths: [runtimeToolsPath] }
	);
	const runtimePath = require.resolve("@module-federation/runtime", {
		paths: [runtimeToolsPath]
	});
	return {
		runtimeTools: runtimeToolsPath,
		bundlerRuntime: bundlerRuntimePath,
		runtime: runtimePath
	};
}

function getDefaultEntryRuntime(
	paths: RuntimePaths,
	options: ModuleFederationPluginOptions,
	compiler: Compiler
) {
	const runtimePlugins = getRuntimePlugins(options);
	const remoteInfos = getRemoteInfos(options);
	const runtimePluginImports = [];
	const runtimePluginVars = [];
	for (let i = 0; i < runtimePlugins.length; i++) {
		const runtimePluginVar = `__module_federation_runtime_plugin_${i}__`;
		runtimePluginImports.push(
			`import ${runtimePluginVar} from ${JSON.stringify(runtimePlugins[i])}`
		);
		runtimePluginVars.push(`${runtimePluginVar}()`);
	}
	const content = [
		`import __module_federation_bundler_runtime__ from ${JSON.stringify(
			paths.bundlerRuntime
		)}`,
		...runtimePluginImports,
		`const __module_federation_runtime_plugins__ = [${runtimePluginVars.join(
			", "
		)}]`,
		`const __module_federation_remote_infos__ = ${JSON.stringify(remoteInfos)}`,
		`const __module_federation_container_name__ = ${JSON.stringify(
			options.name ?? compiler.options.output.uniqueName
		)}`,
		`const __module_federation_share_strategy__ = ${JSON.stringify(
			options.shareStrategy ?? "version-first"
		)}`,
		compiler.webpack.Template.getFunctionContent(
			require("./moduleFederationDefaultRuntime.js")
		)
	].join(";");
	return `@module-federation/runtime/rspack.js!=!data:text/javascript,${content}`;
}
