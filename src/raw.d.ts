// Files imported as text, like `import changelog from "../CHANGELOG.md?raw"`.
declare module "*?raw" {
  const text: string;
  export default text;
}
