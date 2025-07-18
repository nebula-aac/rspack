import { resolve } from "path";
import { readFile, run, runWatch } from "../../utils/test-utils";

describe("build command", () => {
	it.concurrent("it should work ", async () => {
		const { exitCode, stderr, stdout } = await run(__dirname, []);
		expect(exitCode).toBe(0);
		expect(stderr).toBeFalsy();
		expect(stdout).toBeTruthy();
	});
	it.concurrent(
		"should work without command and options (default command)",
		async () => {
			const { exitCode, stderr, stdout } = await run(__dirname, [
				"--mode",
				"development"
			]);

			expect(exitCode).toBe(0);
			expect(stderr).toBeFalsy();
			expect(stdout).toBeTruthy();
		}
	);
	it.concurrent("should work with configuration return function", async () => {
		const { exitCode, stderr, stdout } = await run(__dirname, [
			"--config",
			"./entry.function.js"
		]);
		expect(exitCode).toBe(0);
		expect(stderr).toBeFalsy();
		expect(stdout).toBeTruthy();
	});
	it.concurrent(
		"should pass env.RSPACK_BUILD and env.RSPACK_BUNDLE for function configuration on build mode",
		async () => {
			const { stdout } = await run(__dirname, ["--config", "./entry.env.js"]);
			expect(stdout).toContain("RSPACK_BUILD=true");
			expect(stdout).toContain("RSPACK_BUNDLE=true");
			expect(stdout).not.toContain("RSPACK_WATCH=true");
		}
	);

	it.concurrent(
		"should pass env.RSPACK_WATCH for function configuration on watch mode",
		async () => {
			const { stdout } = await runWatch(
				__dirname,
				["--watch", "--config", "./entry.env.js"],
				{
					// `Rspack compiled successfully` or `Rspack compiled with 1 error`
					killString: /rspack compiled/i
				}
			);
			expect(stdout).not.toContain("RSPACK_BUILD=true");
			expect(stdout).not.toContain("RSPACK_BUNDLE=true");
			expect(stdout).toContain("RSPACK_WATCH=true");
		}
	);
	it.concurrent("should work with configuration return promise", async () => {
		const { exitCode, stderr, stdout } = await run(__dirname, [
			"--config",
			"./entry.promise.js"
		]);
		expect(exitCode).toBe(0);
		expect(stderr).toBeFalsy();
		expect(stdout).toBeTruthy();
	});
	it.concurrent("should work with mjs configuration ", async () => {
		const { exitCode, stderr, stdout } = await run(__dirname, [
			"--config",
			"./entry.config.mjs"
		]);
		expect(exitCode).toBe(0);
		expect(stderr).toBeFalsy();
		expect(stdout).toBeTruthy();
	});
	it("entry option should have higher priority than config", async () => {
		const { exitCode, stderr, stdout } = await run(__dirname, [
			"--entry",
			"./src/other.js",
			"--config",
			"./entry.config.js",
			"--output-path",
			"dist/priority"
		]);
		const mainJs = await readFile(
			resolve(__dirname, "dist/priority/main.js"),
			"utf-8"
		);

		expect(exitCode).toBe(0);
		expect(stderr).toBeFalsy();
		expect(stdout).toBeTruthy();
		expect(mainJs).toContain("other");
		expect(mainJs).not.toContain("CONFIG");
	});

	it.each(["-o", "--output-path", "--outputPath"])(
		"output-path option %p should have higher priority than config",
		async command => {
			const { exitCode, stderr, stdout } = await run(__dirname, [
				command,
				"dist/public",
				"--config",
				"./entry.config.js"
			]);
			const mainJs = await readFile(
				resolve(__dirname, "dist/public/main.js"),
				"utf-8"
			);

			expect(exitCode).toBe(0);
			expect(stderr).toBeFalsy();
			expect(stdout).toBeTruthy();
			expect(mainJs).toContain("CONFIG");
		}
	);

	it.each([
		["-d", "eval-source-map"],
		["--devtool", "eval-source-map"],
		["-d", ""],
		["--devtool", ""],
		["-d", "false"],
		["--devtool", "false"]
	])(
		"devtool option %p should have higher priority than config",
		async (command, option) => {
			const { exitCode, stderr, stdout } = await run(__dirname, [
				command,
				option,
				"--config",
				"./entry.config.js"
			]);

			const mainJs = await readFile(
				resolve(__dirname, "dist/public/main.js"),
				"utf-8"
			);

			expect(exitCode).toBe(0);
			expect(stderr).toBeFalsy();
			expect(stdout).toBeTruthy();
			expect(mainJs).toContain("CONFIG");
		}
	);
});
