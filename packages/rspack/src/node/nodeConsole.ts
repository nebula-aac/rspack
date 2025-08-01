/**
 * The following code is modified based on
 * https://github.com/webpack/webpack/blob/4b4ca3b/lib/node/nodeConsole.js
 *
 * MIT Licensed
 * Author Tobias Koppers @sokra
 * Copyright (c) JS Foundation and other contributors
 * https://github.com/webpack/webpack/blob/main/LICENSE
 */

import * as util from "node:util";
import type { LoggerConsole } from "../logging/createConsoleLogger";
import { truncateArgs } from "../logging/truncateArgs";

export default function ({
	colors,
	appendOnly,
	stream
}: {
	colors?: boolean;
	appendOnly?: boolean;
	stream: NodeJS.WritableStream;
}): LoggerConsole {
	let currentStatusMessage: string[] | undefined;
	let hasStatusMessage = false;
	let currentIndent = "";
	let currentCollapsed = 0;

	const indent = (
		str: string,
		prefix: string,
		colorPrefix: string,
		colorSuffix: string
	): string => {
		if (str === "") return str;
		const prefixWithIndent = currentIndent + prefix;

		if (colors) {
			return (
				prefixWithIndent +
				colorPrefix +
				str.replace(/\n/g, `${colorSuffix}\n${prefix}${colorPrefix}`) +
				colorSuffix
			);
		}
		return prefixWithIndent + str.replace(/\n/g, `\n${prefix}`);
	};

	const clearStatusMessage = () => {
		if (hasStatusMessage) {
			stream.write("\x1b[2K\r");
			hasStatusMessage = false;
		}
	};

	const writeStatusMessage = () => {
		if (!currentStatusMessage) return;
		// cannot be resolved without assertion, copy from webpack
		// Property 'columns' does not exist on type 'WritableStream'.ts(2339)
		const l: number = (stream as unknown as { columns: number }).columns;
		const args = l
			? truncateArgs(currentStatusMessage, l - 1)
			: currentStatusMessage;
		const str = args.join(" ");
		const coloredStr = `\u001b[1m${str}\u001b[39m\u001b[22m`;
		stream.write(`\x1b[2K\r${coloredStr}`);
		hasStatusMessage = true;
	};
	const writeColored = (
		prefix: string,
		colorPrefix: string,
		colorSuffix: string
	): ((...args: any[]) => void) => {
		return (...args) => {
			if (currentCollapsed > 0) return;
			clearStatusMessage();
			const str = indent(
				util.format(...args),
				prefix,
				colorPrefix,
				colorSuffix
			);
			stream.write(`${str}\n`);
			writeStatusMessage();
		};
	};

	const writeGroupMessage = writeColored(
		"<-> ",
		"\u001b[1m\u001b[36m",
		"\u001b[39m\u001b[22m"
	);

	const writeGroupCollapsedMessage = writeColored(
		"<+> ",
		"\u001b[1m\u001b[36m",
		"\u001b[39m\u001b[22m"
	);

	return {
		log: writeColored("    ", "\u001b[1m", "\u001b[22m"),
		debug: writeColored("    ", "", ""),
		trace: writeColored("    ", "", "") as () => void,
		info: writeColored("<i> ", "\u001b[1m\u001b[32m", "\u001b[39m\u001b[22m"),
		warn: writeColored("<w> ", "\u001b[1m\u001b[33m", "\u001b[39m\u001b[22m"),
		error: writeColored("<e> ", "\u001b[1m\u001b[31m", "\u001b[39m\u001b[22m"),
		logTime: writeColored(
			"<t> ",
			"\u001b[1m\u001b[35m",
			"\u001b[39m\u001b[22m"
		),
		group: (...args) => {
			writeGroupMessage(...args);
			if (currentCollapsed > 0) {
				currentCollapsed++;
			} else {
				currentIndent += "  ";
			}
		},
		groupCollapsed: (...args) => {
			writeGroupCollapsedMessage(...args);
			currentCollapsed++;
		},
		groupEnd: () => {
			if (currentCollapsed > 0) currentCollapsed--;
			else if (currentIndent.length >= 2)
				currentIndent = currentIndent.slice(0, currentIndent.length - 2);
		},

		profile: console.profile && (name => console.profile(name)),

		profileEnd: console.profileEnd && (name => console.profileEnd(name)),
		clear: (!appendOnly &&
			console.clear &&
			(() => {
				clearStatusMessage();

				console.clear();
				writeStatusMessage();
			})) as () => void,
		status: appendOnly
			? writeColored("<s> ", "", "")
			: (name, ...argsWithEmpty) => {
					const args = argsWithEmpty.filter(Boolean);
					if (name === undefined && args.length === 0) {
						clearStatusMessage();
						currentStatusMessage = undefined;
					} else if (
						typeof name === "string" &&
						name.startsWith("[webpack.Progress] ")
					) {
						currentStatusMessage = [name.slice(19), ...args];
						writeStatusMessage();
					} else if (name === "[webpack.Progress]") {
						currentStatusMessage = [...args];
						writeStatusMessage();
					} else {
						currentStatusMessage = [name, ...args];
						writeStatusMessage();
					}
				}
	};
}
