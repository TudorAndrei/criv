export function createRequire(): (specifier: string) => never {
  return (specifier: string): never => {
    throw new Error(`Node.js module loading is not available in this webview: ${specifier}`);
  };
}
