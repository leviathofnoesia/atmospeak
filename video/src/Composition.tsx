import {loadFont} from "@remotion/fonts";
import {
  TransitionSeries,
  linearTiming,
} from "@remotion/transitions";
import {fade} from "@remotion/transitions/fade";
import {wipe} from "@remotion/transitions/wipe";
import {
  AbsoluteFill,
  Composition,
  Easing,
  Img,
  interpolate,
  staticFile,
  useCurrentFrame,
} from "remotion";

void Promise.all([
  loadFont({
    family: "Cormorant",
    url: staticFile("fonts/Cormorant-Light.woff2"),
    weight: "300",
  }),
  loadFont({
    family: "Cormorant",
    url: staticFile("fonts/Cormorant-LightItalic.woff2"),
    weight: "300",
    style: "italic",
  }),
  loadFont({
    family: "Geist",
    url: staticFile("fonts/Geist-Regular.woff2"),
    weight: "400",
  }),
  loadFont({
    family: "Geist",
    url: staticFile("fonts/Geist-SemiBold.woff2"),
    weight: "600",
  }),
  loadFont({
    family: "Geist Mono",
    url: staticFile("fonts/GeistMono-Regular.woff2"),
    weight: "400",
  }),
]);

const colors = {
  onyx: "#121113",
  paper: "#f7f7f2",
  dusk: "#485696",
  lilac: "#be95c4",
  teal: "#689689",
  ink: "#1b1a1d",
  softInk: "#66646d",
};

const fps = 30;
const transitionFrames = 18;
const durationInFrames = 150 + 240 + 150 + 180 + 150 - transitionFrames * 4;

const clamp = {
  extrapolateLeft: "clamp" as const,
  extrapolateRight: "clamp" as const,
};

const Kicker = ({children, light = false}: {children: string; light?: boolean}) => (
  <div
    style={{
      fontFamily: "Geist Mono",
      fontSize: 25,
      letterSpacing: "0.24em",
      textTransform: "uppercase",
      color: light ? colors.paper : colors.dusk,
    }}
  >
    + {children}
  </div>
);

const BrandMark = ({inverse = false}: {inverse?: boolean}) => (
  <div style={{display: "flex", alignItems: "center", gap: 18}}>
    <div
      style={{
        width: 58,
        height: 58,
        border: `2px solid ${inverse ? colors.paper : colors.onyx}`,
        borderRadius: "50%",
        display: "grid",
        placeItems: "center",
      }}
    >
      <div
        style={{
          width: 22,
          height: 22,
          borderRadius: "50%",
          background: colors.lilac,
          boxShadow: `0 0 0 8px ${colors.dusk}, 0 0 0 11px ${
            inverse ? colors.paper : colors.onyx
          }`,
        }}
      />
    </div>
    <div>
      <div
        style={{
          fontFamily: "Geist",
          fontWeight: 600,
          fontSize: 30,
          letterSpacing: "0.02em",
          color: inverse ? colors.paper : colors.onyx,
        }}
      >
        Atmospeak
      </div>
      <div
        style={{
          marginTop: 4,
          fontFamily: "Geist Mono",
          fontSize: 15,
          letterSpacing: "0.24em",
          textTransform: "uppercase",
          color: inverse ? "#d7d5df" : colors.softInk,
        }}
      >
        Local · on device
      </div>
    </div>
  </div>
);

const Background = ({dark = false}: {dark?: boolean}) => {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill
      style={{
        background: dark ? colors.onyx : colors.paper,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          position: "absolute",
          inset: 0,
          opacity: dark ? 0.24 : 0.08,
          backgroundImage:
            "linear-gradient(rgba(72,86,150,.28) 1px, transparent 1px), linear-gradient(90deg, rgba(72,86,150,.28) 1px, transparent 1px)",
          backgroundSize: "56px 56px",
          translate: `${interpolate(frame, [0, 240], [0, -56], clamp)}px ${interpolate(
            frame,
            [0, 240],
            [0, -56],
            clamp,
          )}px`,
        }}
      />
      <div
        style={{
          position: "absolute",
          width: 880,
          height: 880,
          right: -260,
          top: -280,
          borderRadius: "50%",
          background: `radial-gradient(circle, ${colors.lilac}55 0%, ${colors.dusk}20 48%, transparent 70%)`,
          scale: interpolate(frame, [0, 150], [0.92, 1.08], clamp),
          opacity: interpolate(frame, [0, 40, 150], [0.5, 0.86, 0.62], clamp),
        }}
      />
    </AbsoluteFill>
  );
};

const Frame = ({
  children,
  dark = false,
  folio,
}: {
  children: React.ReactNode;
  dark?: boolean;
  folio: string;
}) => (
  <AbsoluteFill>
    <Background dark={dark} />
    <div
      style={{
        position: "absolute",
        inset: 48,
        border: `2px solid ${dark ? "#ffffff88" : colors.onyx}`,
        pointerEvents: "none",
      }}
    />
    <div
      style={{
        position: "absolute",
        top: 68,
        left: 84,
        right: 84,
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
      }}
    >
      <BrandMark inverse={dark} />
      <div
        style={{
          fontFamily: "Geist Mono",
          fontSize: 18,
          letterSpacing: "0.18em",
          color: dark ? "#ffffffbb" : colors.softInk,
          textTransform: "uppercase",
        }}
      >
        {folio} · v0.3.1 · Windows x64
      </div>
    </div>
    {children}
  </AbsoluteFill>
);

const Intro = () => {
  const frame = useCurrentFrame();
  return (
    <Frame dark folio="P.01 / Speak">
      <div
        style={{
          position: "absolute",
          left: 120,
          right: 120,
          top: 270,
          display: "flex",
          flexDirection: "column",
          gap: 28,
          opacity: interpolate(frame, [0, 20], [0, 1], {
            ...clamp,
            easing: Easing.bezier(0.16, 1, 0.3, 1),
          }),
          translate: `0 ${interpolate(frame, [0, 24], [70, 0], {
            ...clamp,
            easing: Easing.bezier(0.16, 1, 0.3, 1),
          })}px`,
        }}
      >
        <Kicker light>Desktop dictation · entirely local</Kicker>
        <div
          style={{
            maxWidth: 1480,
            fontFamily: "Cormorant",
            fontWeight: 300,
            fontSize: 174,
            lineHeight: 0.86,
            letterSpacing: "-0.045em",
            color: colors.paper,
          }}
        >
          Speak anywhere.
          <br />
          <span style={{fontStyle: "italic", color: colors.lilac}}>Set the words down.</span>
        </div>
        <div
          style={{
            width: 700,
            fontFamily: "Geist",
            fontSize: 37,
            lineHeight: 1.35,
            color: "#e8e6ed",
            opacity: interpolate(frame, [42, 72], [0, 1], clamp),
          }}
        >
          Your microphone. Your model. Your machine.
        </div>
      </div>
      <div
        style={{
          position: "absolute",
          right: 128,
          bottom: 100,
          width: 210,
          height: 210,
          borderRadius: "50%",
          border: `2px solid ${colors.paper}`,
          backgroundImage: `radial-gradient(${colors.lilac} 2px, transparent 2px)`,
          backgroundSize: "12px 12px",
          opacity: interpolate(frame, [38, 72], [0, 0.58], clamp),
          rotate: `${interpolate(frame, [0, 150], [-8, 8], clamp)}deg`,
        }}
      />
    </Frame>
  );
};

const Waveform = ({frame}: {frame: number}) => (
  <div style={{display: "flex", alignItems: "center", gap: 8, height: 84}}>
    {Array.from({length: 22}).map((_, index) => {
      const energy = 0.3 + 0.7 * Math.abs(Math.sin(frame * 0.16 + index * 0.72));
      return (
        <div
          key={index}
          style={{
            width: 7,
            height: 14 + energy * (index % 4 === 0 ? 58 : 38),
            borderRadius: 6,
            background: index % 3 === 0 ? colors.lilac : "#bec8ff",
          }}
        />
      );
    })}
  </div>
);

const Dock = ({frame}: {frame: number}) => (
  <div
    style={{
      width: 780,
      height: 148,
      borderRadius: 76,
      padding: "0 34px",
      display: "flex",
      alignItems: "center",
      gap: 26,
      color: colors.paper,
      background:
        "linear-gradient(120deg, rgba(38,36,43,.98), rgba(15,14,18,.98))",
      border: "2px solid #77728a",
      boxShadow: `0 28px 80px #12111355, inset 0 1px 0 #ffffff22`,
      scale: interpolate(frame, [0, 14], [0.92, 1], {
        ...clamp,
        easing: Easing.bezier(0.16, 1, 0.3, 1),
      }),
    }}
  >
    <div
      style={{
        width: 74,
        height: 74,
        borderRadius: "50%",
        border: "2px solid #858ecb",
        display: "grid",
        placeItems: "center",
        background: `radial-gradient(circle, ${colors.lilac} 0 14%, ${colors.dusk} 15% 28%, #17161c 29%)`,
        boxShadow: `0 0 ${28 + 10 * Math.sin(frame / 8)}px ${colors.dusk}`,
      }}
    >
      <div style={{width: 12, height: 12, borderRadius: "50%", background: colors.paper}} />
    </div>
    <div style={{flex: 1}}>
      <div
        style={{
          fontFamily: "Geist Mono",
          fontSize: 17,
          letterSpacing: "0.18em",
          textTransform: "uppercase",
          color: "#bcb8c9",
        }}
      >
        Listening · Elgato Wave:3
      </div>
      <Waveform frame={frame} />
    </div>
    <div
      style={{
        fontFamily: "Geist Mono",
        fontSize: 23,
        color: colors.paper,
      }}
    >
      00:{String(Math.floor(frame / fps)).padStart(2, "0")}
    </div>
    <div
      style={{
        width: 68,
        height: 68,
        borderRadius: "50%",
        background: colors.dusk,
        display: "grid",
        placeItems: "center",
        fontSize: 24,
      }}
    >
      ■
    </div>
  </div>
);

const Dictation = () => {
  const frame = useCurrentFrame();
  const fullText =
    "Hi Mara — thanks so much for the studio visit yesterday. I keep thinking about the halftone moon prints.";
  const visibleCharacters = Math.floor(
    interpolate(frame, [58, 185], [0, fullText.length], clamp),
  );
  return (
    <Frame folio="P.02 / Dictate">
      <div
        style={{
          position: "absolute",
          left: 100,
          right: 100,
          top: 190,
          bottom: 88,
          display: "grid",
          gridTemplateColumns: "0.78fr 1.22fr",
          gap: 86,
          alignItems: "center",
        }}
      >
        <div style={{display: "flex", flexDirection: "column", gap: 28}}>
          <Kicker>One shortcut · every app</Kicker>
          <div
            style={{
              fontFamily: "Cormorant",
              fontWeight: 300,
              fontSize: 116,
              lineHeight: 0.9,
              letterSpacing: "-0.04em",
              color: colors.ink,
              opacity: interpolate(frame, [0, 20], [0, 1], clamp),
              translate: `${interpolate(frame, [0, 24], [-45, 0], clamp)}px 0`,
            }}
          >
            Hold.
            <br />
            Speak.
            <br />
            <span style={{fontStyle: "italic", color: colors.dusk}}>Release.</span>
          </div>
          <div
            style={{
              fontFamily: "Geist",
              fontSize: 31,
              lineHeight: 1.4,
              color: colors.softInk,
              opacity: interpolate(frame, [30, 55], [0, 1], clamp),
            }}
          >
            Atmospeak listens only while summoned, then pastes at your cursor.
          </div>
        </div>
        <div
          style={{
            height: 650,
            position: "relative",
            border: `2px solid ${colors.onyx}`,
            background: "#fff",
            boxShadow: `18px 18px 0 ${colors.onyx}`,
            opacity: interpolate(frame, [8, 30], [0, 1], clamp),
            translate: `${interpolate(frame, [8, 34], [60, 0], clamp)}px 0`,
          }}
        >
          <div
            style={{
              height: 76,
              padding: "0 34px",
              borderBottom: "2px solid #dddce0",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              fontFamily: "Geist Mono",
              fontSize: 17,
              letterSpacing: "0.18em",
              color: colors.softInk,
              textTransform: "uppercase",
            }}
          >
            <span>Reply · Re: studio visit</span>
            <span>Draft</span>
          </div>
          <div
            style={{
              padding: "44px 48px",
              display: "flex",
              flexDirection: "column",
              gap: 26,
              fontFamily: "Geist",
              color: colors.ink,
            }}
          >
            <div style={{fontSize: 24, color: colors.softInk}}>To · Mara Okafor</div>
            <div style={{height: 2, background: "#e7e5e8"}} />
            <div
              style={{
                minHeight: 190,
                fontSize: 34,
                lineHeight: 1.48,
              }}
            >
              {fullText.slice(0, visibleCharacters)}
              <span
                style={{
                  display: "inline-block",
                  width: 3,
                  height: 36,
                  marginLeft: 4,
                  verticalAlign: -5,
                  background: colors.dusk,
                  opacity: Math.floor(frame / 10) % 2 ? 0.2 : 1,
                }}
              />
            </div>
          </div>
          <div
            style={{
              position: "absolute",
              left: "50%",
              bottom: 40,
              translate: "-50% 0",
              opacity: interpolate(frame, [22, 40, 192, 218], [0, 1, 1, 0], clamp),
            }}
          >
            <Dock frame={frame} />
          </div>
          <div
            style={{
              position: "absolute",
              right: 30,
              bottom: 26,
              fontFamily: "Geist Mono",
              fontSize: 18,
              letterSpacing: "0.16em",
              textTransform: "uppercase",
              color: colors.teal,
              opacity: interpolate(frame, [194, 220], [0, 1], clamp),
            }}
          >
            ✓ Set down
          </div>
        </div>
      </div>
    </Frame>
  );
};

const Privacy = () => {
  const frame = useCurrentFrame();
  const items = [
    ["01", "Audio stays local"],
    ["02", "Whisper runs on device"],
    ["03", "No account. No cloud."],
  ];
  return (
    <Frame dark folio="P.03 / Private">
      <div
        style={{
          position: "absolute",
          left: 120,
          right: 120,
          top: 225,
          bottom: 100,
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          gap: 100,
          alignItems: "center",
        }}
      >
        <div style={{display: "flex", flexDirection: "column", gap: 26}}>
          <Kicker light>Privacy is the architecture</Kicker>
          <div
            style={{
              fontFamily: "Cormorant",
              fontWeight: 300,
              fontSize: 144,
              lineHeight: 0.88,
              letterSpacing: "-0.045em",
              color: colors.paper,
              opacity: interpolate(frame, [0, 24], [0, 1], clamp),
            }}
          >
            On device.
            <br />
            <span style={{fontStyle: "italic", color: colors.teal}}>By design.</span>
          </div>
        </div>
        <div style={{display: "flex", flexDirection: "column", gap: 18}}>
          {items.map(([number, label], index) => (
            <div
              key={number}
              style={{
                minHeight: 126,
                border: "2px solid #ffffff88",
                padding: "0 32px",
                display: "flex",
                alignItems: "center",
                gap: 28,
                background: index === 1 ? "#48569655" : "#ffffff09",
                opacity: interpolate(frame, [22 + index * 18, 42 + index * 18], [0, 1], clamp),
                translate: `${interpolate(
                  frame,
                  [22 + index * 18, 46 + index * 18],
                  [50, 0],
                  clamp,
                )}px 0`,
              }}
            >
              <span
                style={{
                  fontFamily: "Geist Mono",
                  fontSize: 22,
                  color: colors.lilac,
                }}
              >
                {number}
              </span>
              <span
                style={{
                  fontFamily: "Geist",
                  fontSize: 39,
                  color: colors.paper,
                }}
              >
                {label}
              </span>
            </div>
          ))}
        </div>
      </div>
    </Frame>
  );
};

const Hub = () => {
  const frame = useCurrentFrame();
  const historyOpacity = interpolate(frame, [88, 112], [0, 1], clamp);
  return (
    <Frame folio="P.04 / Remember">
      <div
        style={{
          position: "absolute",
          left: 108,
          right: 108,
          top: 188,
          bottom: 82,
          display: "grid",
          gridTemplateColumns: "0.64fr 1.36fr",
          gap: 72,
          alignItems: "center",
        }}
      >
        <div style={{display: "flex", flexDirection: "column", gap: 24}}>
          <Kicker>History · dictionary · snippets</Kicker>
          <div
            style={{
              fontFamily: "Cormorant",
              fontWeight: 300,
              fontSize: 112,
              lineHeight: 0.92,
              letterSpacing: "-0.04em",
              color: colors.ink,
            }}
          >
            A quiet hub
            <br />
            for every
            <br />
            <span style={{fontStyle: "italic", color: colors.dusk}}>word.</span>
          </div>
          <div
            style={{
              fontFamily: "Geist",
              fontSize: 29,
              lineHeight: 1.45,
              color: colors.softInk,
              opacity: interpolate(frame, [28, 50], [0, 1], clamp),
            }}
          >
            Real transcripts, local metrics, corrections, snippets, and model controls.
          </div>
        </div>
        <div
          style={{
            position: "relative",
            height: 690,
            border: `2px solid ${colors.onyx}`,
            background: "#d9d8d6",
            boxShadow: `20px 20px 0 ${colors.onyx}`,
            overflow: "hidden",
            opacity: interpolate(frame, [0, 22], [0, 1], clamp),
            translate: `${interpolate(frame, [0, 25], [65, 0], clamp)}px 0`,
          }}
        >
          <Img
            src={staticFile("images/hub-home.png")}
            style={{
              position: "absolute",
              width: "100%",
              height: "100%",
              objectFit: "cover",
              opacity: 1 - historyOpacity,
              scale: interpolate(frame, [0, 180], [1.03, 1.09], clamp),
            }}
          />
          <Img
            src={staticFile("images/hub-history.png")}
            style={{
              position: "absolute",
              width: "100%",
              height: "100%",
              objectFit: "cover",
              opacity: historyOpacity,
              scale: interpolate(frame, [88, 180], [1.03, 1.08], clamp),
            }}
          />
          <div
            style={{
              position: "absolute",
              left: 28,
              bottom: 26,
              border: `2px solid ${colors.paper}`,
              padding: "12px 18px",
              background: "#121113dd",
              color: colors.paper,
              fontFamily: "Geist Mono",
              fontSize: 18,
              letterSpacing: "0.16em",
              textTransform: "uppercase",
            }}
          >
            {frame < 100 ? "Home · your daily rhythm" : "History · everything you've said"}
          </div>
        </div>
      </div>
    </Frame>
  );
};

const Outro = () => {
  const frame = useCurrentFrame();
  return (
    <Frame dark folio="P.05 / Begin">
      <div
        style={{
          position: "absolute",
          left: 120,
          right: 120,
          top: 235,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          textAlign: "center",
          gap: 28,
        }}
      >
        <Kicker light>Atmospeak v0.3.1 · Windows x64</Kicker>
        <div
          style={{
            fontFamily: "Cormorant",
            fontSize: 168,
            lineHeight: 0.88,
            letterSpacing: "-0.045em",
            color: colors.paper,
            opacity: interpolate(frame, [0, 24], [0, 1], clamp),
            scale: interpolate(frame, [0, 28], [0.94, 1], {
              ...clamp,
              easing: Easing.bezier(0.16, 1, 0.3, 1),
            }),
          }}
        >
          Your voice,
          <br />
          <span style={{fontStyle: "italic", color: colors.lilac}}>set down.</span>
        </div>
        <div
          style={{
            marginTop: 18,
            border: `2px solid ${colors.paper}`,
            padding: "24px 38px",
            fontFamily: "Geist",
            fontWeight: 600,
            fontSize: 31,
            letterSpacing: "0.04em",
            color: colors.onyx,
            background: colors.lilac,
            boxShadow: `12px 12px 0 ${colors.paper}`,
            opacity: interpolate(frame, [38, 66], [0, 1], clamp),
            translate: `0 ${interpolate(frame, [38, 66], [35, 0], clamp)}px`,
          }}
        >
          DOWNLOAD FOR WINDOWS
        </div>
        <div
          style={{
            fontFamily: "Geist Mono",
            fontSize: 22,
            letterSpacing: "0.11em",
            color: "#d7d5df",
            opacity: interpolate(frame, [62, 88], [0, 1], clamp),
          }}
        >
          leviathofnoesia.github.io/atmospeak
        </div>
      </div>
    </Frame>
  );
};

export const AtmospeakDemo = () => (
  <TransitionSeries>
    <TransitionSeries.Sequence durationInFrames={150} premountFor={fps}>
      <Intro />
    </TransitionSeries.Sequence>
    <TransitionSeries.Transition
      presentation={fade()}
      timing={linearTiming({durationInFrames: transitionFrames})}
    />
    <TransitionSeries.Sequence durationInFrames={240} premountFor={fps}>
      <Dictation />
    </TransitionSeries.Sequence>
    <TransitionSeries.Transition
      presentation={wipe({direction: "from-left"})}
      timing={linearTiming({durationInFrames: transitionFrames})}
    />
    <TransitionSeries.Sequence durationInFrames={150} premountFor={fps}>
      <Privacy />
    </TransitionSeries.Sequence>
    <TransitionSeries.Transition
      presentation={fade()}
      timing={linearTiming({durationInFrames: transitionFrames})}
    />
    <TransitionSeries.Sequence durationInFrames={180} premountFor={fps}>
      <Hub />
    </TransitionSeries.Sequence>
    <TransitionSeries.Transition
      presentation={wipe({direction: "from-bottom"})}
      timing={linearTiming({durationInFrames: transitionFrames})}
    />
    <TransitionSeries.Sequence durationInFrames={150} premountFor={fps}>
      <Outro />
    </TransitionSeries.Sequence>
  </TransitionSeries>
);

export const AtmospeakComposition = () => (
  <Composition
    id="AtmospeakDemo"
    component={AtmospeakDemo}
    durationInFrames={durationInFrames}
    fps={fps}
    width={1920}
    height={1080}
    defaultProps={{}}
  />
);
