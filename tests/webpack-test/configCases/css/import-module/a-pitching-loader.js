/** @type {import("@rspack/core").PitchLoaderDefinitionFunction} */
exports.pitch = async function (remaining) {
	const result = await this.importModule(
	this.resourcePath + '.webpack[javascript/auto]' + '!=!' + remaining, {
	publicPath: ''
	});

	return result.default || result;
};
	