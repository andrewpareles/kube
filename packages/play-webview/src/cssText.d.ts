// esbuild loads CSS files as plain text, which the page hands to Tailwind's compiler.
declare module "*.css" {
  const cssText: string;
  export default cssText;
}
