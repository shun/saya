declare global {
  interface SayaLazyPluginLoadRequest {
    kind: string;
    name: string;
    plugin: string;
    module: string;
    exportName: string;
  }

  interface SayaRuntimePluginSurface {
    loadLazy(request: SayaLazyPluginLoadRequest): Promise<void>;
  }
}

export {};
