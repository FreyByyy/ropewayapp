import { createContext, useContext, type CSSProperties, type SVGProps } from "react";

// Icons by Icons8 (https://icons8.com), one set per theme, fetched with
// scripts/fetch-icons.py. The PNGs are used as masks, so they take the
// current text color, except color sets (Retro), which are drawn as they are.
// The "ropeway" set is the app's own line icons below.

export type IconSetName = "ropeway" | "fluent" | "material" | "puffy" | "glass" | "retro";
/** Sets drawn in their own colors rather than in the text color. */
const COLOR_SETS = new Set<IconSetName>(["retro"]);
export const IconSetContext = createContext<IconSetName>("ropeway");

const PNG = import.meta.glob("./assets/icons/*/*.png", { eager: true, query: "?url", import: "default" }) as Record<
  string,
  string
>;

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function make(name: string, paths: React.ReactNode) {
  return function Icon({ size = 18, className, style, ...rest }: IconProps) {
    const set = useContext(IconSetContext);
    const url = set === "ropeway" ? undefined : PNG[`./assets/icons/${set}/${name}.png`];
    if (url && COLOR_SETS.has(set)) {
      const box: CSSProperties = { width: size, height: size, ...(style as CSSProperties) };
      return <img src={url} alt="" aria-hidden="true" draggable={false} className={`icon-img ${className ?? ""}`} style={box} />;
    }
    if (url) {
      const mask: CSSProperties = {
        width: size,
        height: size,
        WebkitMaskImage: `url(${url})`,
        maskImage: `url(${url})`,
        ...(style as CSSProperties),
      };
      return <span aria-hidden="true" className={`icon-png ${className ?? ""}`} style={mask} />;
    }
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={1.8}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        className={className}
        style={style}
        {...rest}
      >
        {paths}
      </svg>
    );
  };
}

export const IconHash = make("general",
  <>
    <path d="M5 9h14M5 15h14M10 4 8 20M16 4l-2 16" />
  </>,
);
export const IconMonitor = make("monitor", 
  <>
    <rect x="3" y="4" width="18" height="12" rx="2" />
    <path d="M8 20h8M12 16v4" />
  </>,
);
export const IconShield = make("shield", <path d="M12 3 5 6v5c0 4.5 3 8.3 7 10 4-1.7 7-5.5 7-10V6l-7-3Z" />);
export const IconShieldCheck = make("shield-check", 
  <>
    <path d="M12 3 5 6v5c0 4.5 3 8.3 7 10 4-1.7 7-5.5 7-10V6l-7-3Z" />
    <path d="m9 12 2 2 4-4" />
  </>,
);
export const IconSettings = make("settings", 
  <>
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z" />
  </>,
);
export const IconPlus = make("plus", <path d="M12 5v14M5 12h14" />);
export const IconSend = make("send", 
  <>
    <path d="M22 2 11 13" />
    <path d="M22 2 15 22l-4-9-9-4 20-7Z" />
  </>,
);
export const IconCheck = make("check", <path d="m5 12 5 5L20 7" />);
export const IconClock = make("clock", 
  <>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7v5l3 2" />
  </>,
);
export const IconAlert = make("alert", 
  <>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 8v5M12 16h.01" />
  </>,
);
export const IconFiles = make("file", 
  <>
    <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8l-5-5Z" />
    <path d="M14 3v5h5" />
  </>,
);
export const IconCast = make("cast", 
  <>
    <path d="M2 8V6a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-6" />
    <path d="M2 12a9 9 0 0 1 8 8M2 16a5 5 0 0 1 4 4M2 20h.01" />
  </>,
);
export const IconPointer = make("pointer", <path d="m4 4 7 17 2.5-7.5L21 11 4 4Z" />);
export const IconX = make("close", <path d="M18 6 6 18M6 6l12 12" />);
export const IconRadar = make("radar", 
  <>
    <path d="M19.1 4.9A10 10 0 1 0 22 12" />
    <path d="M16.2 7.8A6 6 0 1 0 18 12" />
    <circle cx="12" cy="12" r="2" />
    <path d="m13.4 10.6 6-6" />
  </>,
);

export const IconFolder = make("folder", <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" />);
export const IconDownload = make("download", 
  <>
    <path d="M12 4v11M7 10l5 5 5-5" />
    <path d="M5 20h14" />
  </>,
);
export const IconUpload = make("upload", 
  <>
    <path d="M12 16V5M7 10l5-5 5 5" />
    <path d="M5 20h14" />
  </>,
);
export const IconPencil = make("pencil", 
  <>
    <path d="M4 20h4L19 9l-4-4L4 16v4Z" />
    <path d="m13.5 6.5 4 4" />
  </>,
);
export const IconTrash = make("trash", 
  <>
    <path d="M4 7h16M10 11v6M14 11v6" />
    <path d="M6 7l1 13h10l1-13M9 7V4h6v3" />
  </>,
);
export const IconExternal = make("external", 
  <>
    <path d="M14 4h6v6M20 4l-9 9" />
    <path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" />
  </>,
);
export const IconChevronRight = make("chevron-right", <path d="m9 6 6 6-6 6" />);
export const IconRefresh = make("refresh", 
  <>
    <path d="M20 11a8 8 0 0 0-14.3-4.9L4 8" />
    <path d="M4 4v4h4M4 13a8 8 0 0 0 14.3 4.9L20 16" />
    <path d="M20 20v-4h-4" />
  </>,
);

export const IconMenu = make("menu", <path d="M4 7h16M4 12h16M4 17h16" />);
export const IconPalette = make(
  "palette",
  <>
    <path d="M12 3a9 9 0 1 0 0 18c1.2 0 1.8-.9 1.4-2l-.3-.8c-.4-1.2.5-2.2 1.7-2.2H17a4 4 0 0 0 4-4c0-5-4-9-9-9Z" />
    <circle cx="7.5" cy="11" r="1" />
    <circle cx="10.5" cy="7" r="1" />
    <circle cx="15.5" cy="8" r="1" />
  </>,
);
export const IconChat = make("chat", <path d="M5 5h14a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H9l-4 3v-3H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1Z" />);
export const IconLaptop = make(
  "laptop",
  <>
    <rect x="4" y="5" width="16" height="11" rx="1.5" />
    <path d="M2 19h20" />
  </>,
);
export const IconSun = make(
  "sun",
  <>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
  </>,
);
export const IconMoon = make("moon", <path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z" />);
export const IconSpeaker = make(
  "speaker",
  <>
    <path d="M4 9h4l5-4v14l-5-4H4Z" />
    <path d="M17 9a4 4 0 0 1 0 6" />
  </>,
);
export const IconMute = make(
  "mute",
  <>
    <path d="M4 9h4l5-4v14l-5-4H4Z" />
    <path d="m17 10 4 4M21 10l-4 4" />
  </>,
);
export const IconSmile = make(
  "smile",
  <>
    <circle cx="12" cy="12" r="9" />
    <path d="M8.5 14a4 4 0 0 0 7 0M9 9.5h.01M15 9.5h.01" />
  </>,
);

/** The Ropeway mark: three stations joined by a cable. Takes the text color,
 *  with the top station in the brand teal. */
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 100 100" aria-hidden="true" className="logo">
      <path d="M50 22 L22 74 H78 Z" fill="none" stroke="currentColor" strokeWidth="7" strokeLinejoin="round" />
      <circle cx="50" cy="22" r="12" fill="#6CC8B5" />
      <circle cx="22" cy="74" r="12" fill="currentColor" />
      <circle cx="78" cy="74" r="12" fill="currentColor" />
    </svg>
  );
}
