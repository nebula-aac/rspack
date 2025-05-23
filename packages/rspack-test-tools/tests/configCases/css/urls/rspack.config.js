/** @type {import("@rspack/core").Configuration} */
module.exports = {
	output: {
		cssChunkFilename: "bundle.css"
	},
	target: "web",
	node: {
		__dirname: false,
		__filename: false
	},
	module: {
		generator: {
			"css/auto": {
				exportsOnly: false
			}
		}
	}
};
