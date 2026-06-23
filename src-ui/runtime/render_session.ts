export interface RenderSession {
  initialize(): Promise<void>;
  createWindow(canvas: HTMLCanvasElement, id: string, kind: string, dpr: number, isPrimary: boolean): void;
  resizeWindow(id: string, width: number, height: number): void;
  start(): void;
  stop(): void;
  dispose(): void;
  post(data: any, transfer?: any[]): void;
  onMessage(handler: (msg: any) => void): void;
}


