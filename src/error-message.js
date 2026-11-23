export function errorMessage(error) {
  return typeof error?.message === "string" && !(error instanceof Error) ? error.message : String(error);
}

export function isScreenPermission(error) {
  return error === "screen_permission" || error?.kind === "permission";
}
