const { rspack } = require("@rspack/core");

/** @type {import("@rspack/core").Configuration} */
module.exports = {
	entry: {
		main: ["./index.js"]
	},
	plugins: [
		new rspack.ProvidePlugin({
			str: "./str.js"
		})
	]
};
