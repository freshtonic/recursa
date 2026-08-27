"use strict";

const encoder = new TextEncoder();

export function byteToUtf16Map(source) {
  const byteLength = encoder.encode(source).length;
  const map = new Uint32Array(byteLength + 1);
  let bytes = 0;
  let units = 0;
  for (const scalar of source) {
    const scalarBytes = encoder.encode(scalar).length;
    for (let index = 0; index < scalarBytes; index += 1) map[bytes + index] = units;
    bytes += scalarBytes;
    units += scalar.length;
    map[bytes] = units;
  }
  return map;
}

export function utf16AtByte(map, byteOffset) {
  const safe = Math.max(0, Math.min(Number(byteOffset) || 0, map.length - 1));
  return map[safe];
}

export function spanToUtf16(map, span) {
  return { start: utf16AtByte(map, span.start), end: utf16AtByte(map, span.end) };
}

export function utf8ByteAtUtf16(source, utf16Offset) {
  let safe = Math.max(0, Math.min(source.length, utf16Offset));
  if (safe > 0 && safe < source.length) {
    const previous = source.charCodeAt(safe - 1);
    const next = source.charCodeAt(safe);
    if (previous >= 0xd800 && previous <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) safe -= 1;
  }
  return encoder.encode(source.slice(0, safe)).length;
}

export function partitionDecorations(source, decorations) {
  const map = byteToUtf16Map(source);
  const converted = decorations
    .filter((item) => item.span && item.span.end >= item.span.start)
    .map((item) => ({ ...item, range: spanToUtf16(map, item.span) }));
  const boundaries = new Set([0, source.length]);
  for (const item of converted) {
    boundaries.add(item.range.start);
    boundaries.add(item.range.end);
  }
  const sorted = [...boundaries].sort((left, right) => left - right);
  const result = [];
  for (let index = 0; index < sorted.length - 1; index += 1) {
    const start = sorted[index];
    const end = sorted[index + 1];
    const covering = converted.filter((item) => item.range.start <= start && item.range.end >= end);
    result.push({ text: source.slice(start, end), start, end, decorations: covering });
  }
  return result;
}

export function parseHexSeed(value) {
  const compact = value.replace(/\s+/gu, "").replace(/^0x/u, "");
  if (!compact || compact.length % 2 !== 0 || !/^[0-9a-f]+$/iu.test(compact)) {
    throw new Error("Use an even number of hexadecimal digits (for example 0711a0ff).");
  }
  if (compact.length > 8192) throw new Error("Seed may contain at most 4096 bytes.");
  const bytes = [];
  for (let index = 0; index < compact.length; index += 2) {
    bytes.push(Number.parseInt(compact.slice(index, index + 2), 16));
  }
  return bytes;
}

export function deepestContaining(entries, byteOffset) {
  const ranked = (candidates) => candidates
    .sort((left, right) =>
      right.depth - left.depth ||
      (left.span.end - left.span.start) - (right.span.end - right.span.start) ||
      left.order - right.order
    )[0] || null;
  return ranked(entries.filter((entry) => entry.span && entry.span.start <= byteOffset &&
    (entry.span.start === entry.span.end ? byteOffset === entry.span.start : byteOffset < entry.span.end))) ||
    ranked(entries.filter((entry) => entry.span && entry.span.end === byteOffset));
}

export function mappedSourceSpan(sourceMap, rendered, utf16Offset) {
  const byteOffset = utf8ByteAtUtf16(rendered, utf16Offset);
  const ranked = (entries) => entries
    .sort((left, right) =>
      (left.output.end - left.output.start) - (right.output.end - right.output.start) ||
      left.output.start - right.output.start
    )[0]?.source || null;
  return ranked(sourceMap.filter((entry) => entry.output.start <= byteOffset &&
    (entry.output.start === entry.output.end ? byteOffset === entry.output.start : byteOffset < entry.output.end))) ||
    ranked(sourceMap.filter((entry) => entry.output.end === byteOffset));
}
