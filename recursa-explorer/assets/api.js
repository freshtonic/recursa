"use strict";

async function responseJson(response) {
  const value = await response.json();
  if (!response.ok) {
    throw new Error(value?.error?.message || `Explorer request failed (${response.status})`);
  }
  return value;
}

export async function getSchema(signal) {
  return responseJson(await fetch("/api/schema", { signal, credentials: "same-origin" }));
}

export async function post(operation, body, signal) {
  const response = await fetch(`/api/${operation}`, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
    signal,
  });
  return responseJson(response);
}
