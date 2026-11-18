import { createContext, useContext } from "react";

export const ROLE_LABEL: Record<string, string> = { owner: "Owner", admin: "Admin", reviewer: "Reviewer", viewer: "Viewer" };

/** Seitenleiste neu laden, z. B. nach Anlegen oder Einlesen eines Reviews. */
export const RefreshContext = createContext<() => void>(() => {});
export const useRefreshSidebar = () => useContext(RefreshContext);
