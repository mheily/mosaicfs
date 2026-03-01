// Stub for Node's `node:async_hooks` used by react-router's server-side code.
// This module is imported by react-router's development bundle but the code
// paths that call it are never reached in a browser build.
export const AsyncLocalStorage = class {};
export const AsyncResource = class {};
export const asyncLocalStorage = null;
export default {};
