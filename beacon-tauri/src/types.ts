export interface VersionEntry {
  id: string;
  type: string;
  url: string;
  time: string;
  releaseTime: string;
  sha1: string;
  complianceLevel: number;
}

export type Account =
  | { type: "Offline"; username: string; uuid: string }
  | { type: "Microsoft"; id: string; username: string; uuid: string };

export interface DownloadProgress {
  phase: string;
  files_done: number;
  files_total: number;
  bytes_done: number;
  bytes_total: number;
  downloaded_done: number;
  current_file: string | null;
}

export type LaunchStatus = "launching" | "running" | "exited";

export interface LaunchStatusEvent {
  instanceId: string;
  status: LaunchStatus;
}

export interface DeviceAuthorization {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
  message: string;
}

export type ModLoaderKind = "Fabric" | "Forge" | "NeoForge" | "Quilt";

export interface ModLoaderInfo {
  kind: ModLoaderKind;
  loader_version: string;
  effective_version_id: string;
}

export interface Instance {
  id: string;
  name: string;
  version_id: string;
  icon_path: string | null;
  pinned_screenshot: string | null;
  mod_loader: ModLoaderInfo | null;
  dir: string;
  mods_dir: string;
  saves_dir: string;
  resource_packs_dir: string;
  shader_packs_dir: string;
  screenshots_dir: string;
}

export interface LoaderVersionInfo {
  version: string;
  stable: boolean;
  recommended: boolean;
}

export type ModSource = "Modrinth" | "CurseForge";

// The mod browser, resource-pack browser, and shader-pack browser all search/preview/install
// against the same two sources -- `ContentKind` is what tells the shared backend commands (and the
// shared browser modal) which of the three folders/facets/classes to use. The `Mod*`-named types
// below are shaped identically for all three kinds (only ever "a searchable download"), so they're
// reused as-is rather than renamed.
export type ContentKind = "Mod" | "ResourcePack" | "ShaderPack";

export interface ModSearchResult {
  id: string;
  source: ModSource;
  title: string;
  author: string;
  description: string;
  icon_url: string | null;
  downloads: number;
}

export interface ModVersionOption {
  id: string;
  version_number: string;
  filename: string;
  is_stable: boolean;
}

export interface ModInstallPreviewEntry {
  project_id: string;
  title: string;
  filename: string;
  version_number: string;
  is_dependency: boolean;
}

export interface ModProvenanceEntry {
  source: ModSource;
  projectId: string;
  filename: string;
}

export interface WorldInfo {
  name: string;
  datapacks: string[];
  icon_data_url: string | null;
}

export interface ModInfo {
  name: string;
  enabled: boolean;
  version: string | null;
  icon_data_url: string | null;
}

export interface ResourcePackInfo {
  name: string;
  icon_data_url: string | null;
}

export interface ScreenshotInfo {
  name: string;
  path: string;
}

export interface SkinInfo {
  id: string;
  state: string;
  url: string;
  variant: string;
}

export interface CapeInfo {
  id: string;
  state: string;
  url: string;
  alias: string;
}

export interface MinecraftProfile {
  id: string;
  name: string;
  skins: SkinInfo[];
  capes: CapeInfo[];
}

export interface InstancesResponse {
  instances: Instance[];
  selected_id: string | null;
}

export interface DirectorySettings {
  game_dir: string;
  instances_dir: string;
  config_dir: string;
  libraries_dir: string;
}
