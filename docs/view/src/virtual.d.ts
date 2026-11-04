declare module 'virtual:fogu-examples' {
  import type { ExampleFile } from '../examples-plugin';
  const files: ExampleFile[];
  export default files;
}
declare module 'virtual:fogu-examples-code' {
  const code: Record<string, string>;
  export default code;
}
