// SvelteKit global type augmentation entrypoint.

declare global {
  namespace App {}

  // CSS module declarations
  declare module '*.css' {
    const content: string;
    export default content;
  }
}
