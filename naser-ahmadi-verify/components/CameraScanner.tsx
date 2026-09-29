"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { Icon } from "./Icon";

export type ScanSide = { key: string; label: string; hint: string };

// Live camera viewfinder with a card-shaped guide. The patient lines the card
// up and taps the shutter; the photo is cropped to the guide and handed to
// onCapture. With `detect`, frames are also checked continuously for the
// license barcode and captured automatically as soon as it reads.
export function CameraScanner({ kind, side, done, onCapture, onUnavailable, detect }: {
  kind: "license" | "insurance";
  side: ScanSide | null; // the side being scanned; null once every side is captured
  done: boolean;
  onCapture: (key: string, file: File, detected?: string) => void;
  onUnavailable: (reason: string) => void;
  detect?: (frame: ImageData) => Promise<string | null>;
}) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const [ready, setReady] = useState(false);
  const [flash, setFlash] = useState(false);
  const [auto, setAuto] = useState(false);
  const capturing = useRef(false);

  // Start the rear camera while there is a side left to scan; stop it after.
  useEffect(() => {
    if (!side) return;
    let cancelled = false;
    (async () => {
      if (!navigator.mediaDevices?.getUserMedia) return onUnavailable("This browser can't open the camera.");
      try {
        const stream = await navigator.mediaDevices.getUserMedia({
          audio: false,
          video: { facingMode: { ideal: "environment" }, width: { ideal: 1920 }, height: { ideal: 1080 } },
        });
        if (cancelled) return stream.getTracks().forEach((t) => t.stop());
        streamRef.current = stream;
        const v = videoRef.current!;
        v.srcObject = stream;
        await v.play().catch(() => {});
        setReady(true);
      } catch (e) {
        const denied = e instanceof DOMException && e.name === "NotAllowedError";
        onUnavailable(denied ? "Camera access was blocked." : "We couldn't open the camera.");
      }
    })();
    return () => {
      cancelled = true;
      streamRef.current?.getTracks().forEach((t) => t.stop());
      streamRef.current = null;
      setReady(false);
    };
    // Restart only when switching between "scanning" and "all done".
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [Boolean(side)]);

  // Draws the part of the video inside the card guide onto a canvas.
  const grab = useCallback((maxWidth = 1800): HTMLCanvasElement | null => {
    const v = videoRef.current, box = boxRef.current;
    if (!v || !box || !v.videoWidth) return null;
    const bw = box.clientWidth, bh = box.clientHeight;
    // The video is drawn with object-fit: cover — map box pixels to video pixels.
    const scale = Math.max(bw / v.videoWidth, bh / v.videoHeight);
    const offX = (bw - v.videoWidth * scale) / 2, offY = (bh - v.videoHeight * scale) / 2;
    // Crop to the card guide (88% of the box width, see .app-cam__guide) plus a small margin.
    const gw = bw * 0.94, gh = Math.min(bh * 0.94, gw / 1.586);
    const gx = (bw - gw) / 2, gy = (bh - gh) / 2;
    const sx = (gx - offX) / scale, sy = (gy - offY) / scale, sw = gw / scale, sh = gh / scale;
    const out = Math.min(maxWidth, Math.round(sw));
    const c = document.createElement("canvas");
    c.width = out;
    c.height = Math.round(out * (sh / sw));
    c.getContext("2d")!.drawImage(v, sx, sy, sw, sh, 0, 0, c.width, c.height);
    return c;
  }, []);

  const shoot = useCallback(async (detected?: string) => {
    if (!side || capturing.current) return;
    capturing.current = true;
    const c = grab();
    if (!c) { capturing.current = false; return; }
    setFlash(true);
    setTimeout(() => setFlash(false), 180);
    navigator.vibrate?.(30);
    const blob = await new Promise<Blob | null>((r) => c.toBlob(r, "image/jpeg", 0.92));
    capturing.current = false;
    if (blob) onCapture(side.key, new File([blob], `${side.key}.jpg`, { type: "image/jpeg" }), detected);
  }, [grab, onCapture, side]);

  // Continuous barcode detection for sides that support it.
  useEffect(() => {
    if (!ready || !side || !detect) { setAuto(false); return; }
    setAuto(true);
    let stop = false;
    (async () => {
      while (!stop) {
        const c = grab(1400);
        if (c) {
          const text = await detect(c.getContext("2d")!.getImageData(0, 0, c.width, c.height)).catch(() => null);
          if (stop) break;
          if (text) { await shoot(text); break; }
        }
        await new Promise((r) => setTimeout(r, 250));
      }
    })();
    return () => { stop = true; };
  }, [ready, side, detect, grab, shoot]);

  return (
    <div ref={boxRef} className={`app-cam app-cam--${kind} ${done ? "is-done" : ""}`}>
      <video ref={videoRef} playsInline muted autoPlay className={ready && side ? "is-live" : ""} />
      {side && (
        <>
          <div className="app-cam__guide"><i /><i /><i /><i /></div>
          {ready && <div className="app-cam__scan" />}
          <div className="app-cam__chip">
            {kind === "license" ? "License" : "Insurance card"} · <b>{side.label}</b>
            {auto && <span className="app-cam__auto"> · auto-scanning</span>}
          </div>
          <div className="app-cam__hint">{ready ? side.hint : "Starting camera…"}</div>
          <button className="app-cam__shutter" aria-label={`Capture ${side.label.toLowerCase()}`} disabled={!ready} onClick={() => shoot()}>
            <span />
          </button>
        </>
      )}
      {done && (
        <div className="app-cam__done">
          <span className="success-badge" style={{ width: 56, height: 56 }}><Icon name="check" size={28} sw={3} /></span>
          <div>Both sides captured</div>
          <small>Tap a photo below to retake it</small>
        </div>
      )}
      {flash && <div className="app-cam__flash" />}
    </div>
  );
}
