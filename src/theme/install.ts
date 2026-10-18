import { createContext, useContext } from "react";

/** Installs a theme from a `.ropeway-theme` file (asks the user first). */
export const ThemeInstallContext = createContext<(path: string) => void>(() => {});
export const useThemeInstall = () => useContext(ThemeInstallContext);

export const THEME_EXT = "ropeway-theme";
export const isThemeFile = (name: string | undefined | null) => !!name && name.toLowerCase().endsWith(`.${THEME_EXT}`);
