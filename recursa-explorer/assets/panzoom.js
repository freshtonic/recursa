"use strict";

const MIN_ZOOM = 0.3;
const MAX_ZOOM = 1.4;

function clamp(value, min, max) { return Math.min(max, Math.max(min, value)); }

/// Guards a computed geometry value against non-finite results (an empty field, a zero-size
/// document) so a stray `NaN` never reaches a `translate()` or SVG path, per the design spec.
export function fin(value, fallback) { return Number.isFinite(value) ? value : fallback; }

/// Shared drag-to-pan / wheel-to-zoom controller for the Constellation and Nest fields. `state`
/// is the mode's own persistent `{x, y, z}` triple, so switching modes and back preserves it.
export function createPanZoom(field, layer, state, onChange) {
  function apply() {
    layer.style.transform = `translate(${fin(state.x, 0)}px, ${fin(state.y, 0)}px) scale(${fin(state.z, 1)})`;
    onChange?.(state);
  }
  apply();

  let dragging = false;
  let start = { x: 0, y: 0, panX: 0, panY: 0 };

  function onPointerDown(event) {
    dragging = true;
    start = { x: event.clientX, y: event.clientY, panX: state.x, panY: state.y };
    field.setPointerCapture(event.pointerId);
  }
  function onPointerMove(event) {
    if (!dragging) return;
    state.x = start.panX + (event.clientX - start.x);
    state.y = start.panY + (event.clientY - start.y);
    apply();
  }
  function onPointerUp() { dragging = false; }
  function onWheel(event) {
    event.preventDefault();
    zoomBy(event.deltaY < 0 ? 1.08 : 1 / 1.08, event.clientX, event.clientY);
  }

  field.addEventListener("pointerdown", onPointerDown);
  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", onPointerUp);
  field.addEventListener("wheel", onWheel, { passive: false });

  function zoomBy(factor, clientX, clientY) {
    const rect = field.getBoundingClientRect();
    const anchorX = clientX != null ? clientX - rect.left : rect.width / 2;
    const anchorY = clientY != null ? clientY - rect.top : rect.height / 2;
    const nextZ = fin(clamp(state.z * factor, MIN_ZOOM, MAX_ZOOM), state.z);
    const ratio = nextZ / state.z;
    state.x = anchorX - (anchorX - state.x) * ratio;
    state.y = anchorY - (anchorY - state.y) * ratio;
    state.z = nextZ;
    apply();
  }

  function fit(contentWidth, contentHeight) {
    const rect = field.getBoundingClientRect();
    const z = fin(clamp(
      Math.min((rect.width - 36) / Math.max(1, contentWidth), (rect.height - 36) / Math.max(1, contentHeight)),
      MIN_ZOOM, MAX_ZOOM,
    ), 1);
    state.z = z;
    state.x = fin((rect.width - contentWidth * z) / 2, 0);
    state.y = fin((rect.height - contentHeight * z) / 2, 0);
    apply();
  }

  return {
    zoomIn: () => zoomBy(1.2),
    zoomOut: () => zoomBy(1 / 1.2),
    fit,
    destroy() {
      field.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      field.removeEventListener("wheel", onWheel);
    },
  };
}
