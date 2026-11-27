import { useEffect, useRef, useState, type ReactNode } from "react";
import type { PDFDocumentLoadingTask, PDFDocumentProxy, RenderTask } from "pdfjs-dist";
import { Button, Spinner } from "../../ui/kit.tsx";

export function PdfExportView({ blob, name, note, children }: { blob: Blob; name: string; note: string | null; children: ReactNode }) {
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(null);
  const [page, setPage] = useState(1);
  const [error, setError] = useState("");
  const [drawn, setDrawn] = useState(false);
  const canvas = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    let live = true;
    let task: PDFDocumentLoadingTask | null = null;
    void (async () => {
      const [{ getDocument, GlobalWorkerOptions }, data] = await Promise.all([
        import("pdfjs-dist/legacy/build/pdf.mjs"), blob.arrayBuffer(),
      ]);
      if (!live) return;

      const assets = new URL("pdfjs/", location.href);
      GlobalWorkerOptions.workerSrc = new URL("pdf-worker.js", assets).href;
      task = getDocument({
        data, cMapUrl: new URL("cmaps/", assets).href, cMapPacked: true,
        iccUrl: new URL("iccs/", assets).href, standardFontDataUrl: new URL("standard_fonts/", assets).href,
        wasmUrl: new URL("wasm/", assets).href, enableHWA: false, disableWorkerRendering: true,
      });
      const document = await task.promise;
      if (live) setPdf(document);
    })().catch(() => { if (live) setError("Could not open the PDF preview."); });

    return () => {
      live = false;
      void task?.destroy().catch(() => {});
    };
  }, [blob]);

  useEffect(() => {
    const element = canvas.current;
    if (!pdf || !element) return;
    let live = true;
    let render: RenderTask | null = null;
    void (async () => {
      const source = await pdf.getPage(page);
      if (!live) return;

      const base = source.getViewport({ scale: 1 });
      const scale = Math.min(3, 2048 / Math.max(base.width, base.height));
      const viewport = source.getViewport({ scale });
      element.width = Math.max(1, Math.floor(viewport.width));
      element.height = Math.max(1, Math.floor(viewport.height));
      render = source.render({ canvas: element, viewport });
      await render.promise;
      if (live) setDrawn(true);
    })().catch(() => { if (live) setError("Could not draw this PDF page."); });

    return () => { live = false; render?.cancel(); };
  }, [pdf, page]);

  function turn(to: number) {
    setDrawn(false);
    setError("");
    setPage(to);
  }

  return <><div className="agpstage pdf-export">
    {error ? <div className="agpmsg derr">{error}</div> : !drawn && <div className="agpmsg"><Spinner />Opening PDF…</div>}
    <canvas ref={canvas} hidden={!drawn || !!error} role="img" aria-label={`${name}, page ${page}`} />
  </div><div className="agpacts">
    {pdf && pdf.numPages > 1 && <>
      <Button disabled={page <= 1} onClick={() => turn(page - 1)}>Previous</Button>
      <span className="meta">{page} / {pdf.numPages}</span>
      <Button disabled={page >= pdf.numPages} onClick={() => turn(page + 1)}>Next</Button>
    </>}
    <span className="meta">{note}</span><span className="sp" />{children}
  </div></>;
}
