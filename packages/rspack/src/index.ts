import * as rspackExports from "./exports";
import { rspack as rspackFn } from "./rspack";

// add exports on rspack() function
type Rspack = typeof rspackFn &
	typeof rspackExports & { rspack: Rspack; webpack: Rspack };
const fn = Object.assign(rspackFn, rspackExports) as Rspack;
fn.rspack = fn;
fn.webpack = fn;
const rspack: Rspack = fn;

export * from "./exports";
export default rspack;
export { rspack };
