// Taking a picture with the machine's own camera, from inside the image
// picker.
//
// The preview takes the picker's place rather than opening a dialog of its
// own: the picker is already inside one, and the modal host holds a single
// dialog, so a second would have to tear down the entry editor and put it back
// afterwards.

import { importImageBytes } from "./api";
import { fileStamp } from "./dates";
import { el, focusWhenActive, toast, toastError } from "./ui";

/**
 * The stream currently feeding a preview, if any.
 *
 * Held here rather than in the view because the view can be taken off the page
 * without passing through Cancel — a rescan rebuilds the picker, Escape closes
 * the whole dialog — and a stream nobody stopped keeps the camera light on.
 */
let live: MediaStream | null = null;

/** Whether this machine offers a camera at all. */
export const cameraAvailable = (): boolean =>
  typeof navigator.mediaDevices?.getUserMedia === "function";

/** Release the camera, wherever the preview went. */
export function stopCamera(): void {
  for (const track of live?.getTracks() ?? []) track.stop();
  live = null;
}

export interface CameraOptions {
  /** Entry id, or null for the project cover. */
  entryId: string | null;
  /** Called once the photo is on disk. */
  onChanged: () => void;
  /** Put the picker back: after the shot, after Cancel, and after a failure. */
  onClosed: () => void;
}

/**
 * Show the live preview in `host`, replacing whatever was in it.
 *
 * The camera is asked for before anything is drawn, so a refusal leaves the
 * picker as it was rather than flashing an empty frame first.
 */
export async function openCamera(
  host: HTMLElement,
  options: CameraOptions,
): Promise<void> {
  stopCamera();
  const asking = el("p", { class: "hint", text: "Waiting for the camera…" });
  host.replaceChildren(asking);

  let stream: MediaStream;
  try {
    stream = await startStream();
  } catch (err) {
    toast(cameraProblem(err), { error: true, duration: null });
    options.onClosed();
    return;
  }
  // Cancelled while the permission prompt was up: the picker is already back,
  // so the stream has nowhere to go.
  if (!host.isConnected) {
    for (const track of stream.getTracks()) track.stop();
    return;
  }
  live = stream;

  const video = el("video", {
    class: "camera__view",
    // Muted and inline, or Chromium declines to play it on its own.
    autoplay: "",
    playsinline: "",
    muted: "",
  }) as HTMLVideoElement;
  video.muted = true;
  video.srcObject = stream;

  const close = () => {
    stopCamera();
    options.onClosed();
  };

  const shutter = el("button", {
    class: "button button--primary",
    text: "Take photo",
    onclick: () => void capture(video, options, close),
  });

  const view = el(
    "div",
    { class: "camera" },
    video,
    el(
      "div",
      { class: "camera__actions" },
      shutter,
      el("button", { class: "button", text: "Cancel", onclick: close }),
      // Only worth a button when there is somewhere to switch to. The labels
      // that would let it name the cameras are only readable once one of them
      // has been granted, which is why this is asked for after the stream.
      await switchButton(host, options),
    ),
  );
  host.replaceChildren(view);
  // The shutter takes the focus so Space and Enter fire it — and so Enter does
  // not reach the note behind, where it means "done, close the editor".
  focusWhenActive(shutter);
}

/**
 * The camera to use next time, or null for whichever one the browser picks.
 *
 * Kept across openings so that switching to the good camera is done once per
 * session rather than once per photo.
 */
let preferredDevice: string | null = null;

function startStream(): Promise<MediaStream> {
  return navigator.mediaDevices.getUserMedia({
    audio: false,
    // A hint, not a demand: `ideal` falls back to whatever the camera does
    // offer, where `exact` would fail outright on a 720p webcam.
    video: preferredDevice
      ? { deviceId: { exact: preferredDevice } }
      : { width: { ideal: 1920 }, height: { ideal: 1080 } },
  });
}

/** A button that cycles through the cameras, or nothing when there is one. */
async function switchButton(
  host: HTMLElement,
  options: CameraOptions,
): Promise<HTMLElement | null> {
  let cameras: MediaDeviceInfo[] = [];
  try {
    const devices = await navigator.mediaDevices.enumerateDevices();
    cameras = devices.filter((device) => device.kind === "videoinput");
  } catch {
    // Not being able to list them is not a reason to lose the preview that is
    // already running.
    return null;
  }
  if (cameras.length < 2) return null;

  return el("button", {
    class: "button",
    text: "Switch camera",
    onclick: () => {
      const at = cameras.findIndex((c) => c.deviceId === currentDevice());
      preferredDevice =
        cameras[(at + 1) % cameras.length]?.deviceId ?? preferredDevice;
      void openCamera(host, options);
    },
  });
}

/** Which camera the live stream is actually coming from. */
function currentDevice(): string | null {
  return live?.getVideoTracks()[0]?.getSettings().deviceId ?? preferredDevice;
}

/**
 * Save what the preview is showing.
 *
 * The frame is drawn mirrored, because the preview is mirrored: the picture
 * that lands in the entry is the one that was on screen when the button was
 * pressed.
 */
async function capture(
  video: HTMLVideoElement,
  options: CameraOptions,
  close: () => void,
): Promise<void> {
  const width = video.videoWidth;
  const height = video.videoHeight;
  if (width === 0 || height === 0) {
    toast("The camera has not started yet — try again in a moment.");
    return;
  }

  const canvas = el("canvas") as HTMLCanvasElement;
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  if (!context) {
    toast("This machine cannot draw the photo.", { error: true });
    close();
    return;
  }
  context.translate(width, 0);
  context.scale(-1, 1);
  context.drawImage(video, 0, 0, width, height);

  // The camera is released before the write: the picture is taken, and holding
  // it open through a disk write only keeps the light on longer.
  close();

  const blob = await new Promise<Blob | null>((resolve) =>
    // JPEG rather than PNG: a photograph, where PNG costs several megabytes a
    // shot for nothing the eye can find.
    canvas.toBlob(resolve, "image/jpeg", 0.92),
  );
  if (!blob) {
    toast("The photo could not be encoded.", { error: true, duration: null });
    return;
  }

  try {
    const bytes = new Uint8Array(await blob.arrayBuffer());
    const saved = await importImageBytes(options.entryId, photoName(), bytes);
    toast(`Added ${saved}`);
    options.onChanged();
  } catch (err) {
    toastError("Could not save the photo", err);
  }
}

/** `photo-2026-09-17T14-05-31.jpg`, in the shape pasted images already use. */
function photoName(): string {
  return `photo-${fileStamp()}.jpg`;
}

/** What went wrong, in the words of someone who wanted a photograph. */
function cameraProblem(err: unknown): string {
  const name = err instanceof Error ? err.name : "";
  if (name === "NotAllowedError")
    return "Lazuli was not allowed to use the camera. Windows and the app both have to permit it.";
  if (name === "NotFoundError" || name === "OverconstrainedError")
    return "No camera was found on this machine.";
  if (name === "NotReadableError")
    return "The camera is already in use by another app.";
  return `The camera could not be started: ${String(err)}`;
}
