// Typed wrappers around the Rust commands in src-tauri/src/lib.rs.
// Keep these types in step with the Rust structs (serde camelCase).
import { invoke } from "@tauri-apps/api/core";

export type Check = {
  id: string;
  label: string;
  ok: boolean;
  detail: string;
  fix: string | null;
};

export type DoctorReport = {
  checks: Check[];
  childPath: string;
};

export const doctor = () => invoke<DoctorReport>("doctor");
