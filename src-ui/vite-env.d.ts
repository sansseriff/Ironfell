/// <reference types="svelte" />
/// <reference types="vite/client" />


// Add support for WASM imports
declare module '*.wasm' {
    const src: string;
    export default src;
}

declare module '*.wasm?url' {
    const src: string;
    export default src;
}

/** The commit the frontend was served or built from (vite.config.ts). */
declare const __IRON_FRONTEND_VERSION__: string;
