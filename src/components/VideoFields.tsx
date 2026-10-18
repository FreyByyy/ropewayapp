import type { Quality, Resolution, VideoSettings } from "../api";
import { useI18n } from "../i18n";

const RESOLUTIONS: Resolution[] = ["auto", "native", "1440", "1080", "720"];
const FPS = [15, 30, 60];
const QUALITIES: Quality[] = ["economy", "standard", "high"];

/** Resolution, frame rate and quality pickers for screen sharing. */
export function VideoFields({
  value,
  onChange,
  dark,
}: {
  value: VideoSettings;
  onChange: (v: VideoSettings) => void;
  /** For the viewer window's dark panel. */
  dark?: boolean;
}) {
  const { t } = useI18n();
  const resLabel = (r: Resolution) => (r === "auto" ? t("resAuto") : r === "native" ? t("resNative") : `${r}p`);
  const qLabel = (q: Quality) => (q === "economy" ? t("qEconomy") : q === "high" ? t("qHigh") : t("qStandard"));

  return (
    <div className={`video-fields ${dark ? "dark" : ""}`}>
      <label>
        <span>{t("resolution")}</span>
        <select value={value.resolution} onChange={(e) => onChange({ ...value, resolution: e.target.value as Resolution })}>
          {RESOLUTIONS.map((r) => (
            <option key={r} value={r}>
              {resLabel(r)}
            </option>
          ))}
        </select>
      </label>
      <label>
        <span>{t("frameRate")}</span>
        <select value={value.fps} onChange={(e) => onChange({ ...value, fps: Number(e.target.value) })}>
          {FPS.map((f) => (
            <option key={f} value={f}>
              {t("fpsValue", { n: f })}
            </option>
          ))}
        </select>
      </label>
      <label>
        <span>{t("quality")}</span>
        <select value={value.quality} onChange={(e) => onChange({ ...value, quality: e.target.value as Quality })}>
          {QUALITIES.map((q) => (
            <option key={q} value={q}>
              {qLabel(q)}
            </option>
          ))}
        </select>
      </label>
      {!dark && <p className="field-hint video-hint">{t("videoHint")}</p>}
    </div>
  );
}
