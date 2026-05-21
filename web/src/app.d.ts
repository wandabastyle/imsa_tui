// SvelteKit global type augmentation entrypoint.

declare global {
  namespace App {}
}

declare module '*.css' {
  const content: string;
  export default content;
}
