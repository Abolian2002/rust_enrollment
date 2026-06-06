import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./app/**/*.{ts,tsx}", "./components/**/*.{ts,tsx}", "./lib/**/*.{ts,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        sans: ["var(--font-sans)", "system-ui", "sans-serif"],
        serif: ["var(--font-serif)", "Georgia", "serif"],
      },
      colors: {
        ink: {
          50: "#fbf8f1",
          100: "#f0ebe1",
          200: "#dad1bf",
          300: "#b5a88e",
          400: "#92856c",
          500: "#756952",
          600: "#5e5440",
          700: "#494030",
          800: "#383124",
          900: "#141413"
        },
        school: {
          50: "#f2f5fb",
          100: "#e0e8f4",
          200: "#c7d5e9",
          300: "#a3badd",
          400: "#7b99cc",
          500: "#5a7ebb",
          600: "#446399",
          700: "#364e7a",
          800: "#1a2a47",
          900: "#08172e"
        },
        gold: {
          50: "#fdf8ee",
          100: "#f8eecd",
          200: "#f2e09b",
          300: "#e9cf64",
          400: "#e0bb37",
          500: "#d2a758",
          600: "#a67f1a",
          700: "#856011",
          800: "#6e4d15",
          900: "#5e4118"
        }
      },
      boxShadow: {
        soft: "0 24px 64px rgba(8, 23, 46, 0.08)",
        sharp: "4px 4px 0px rgba(8, 23, 46, 1)",
      },
      backgroundImage: {
        "academy-texture": "url(\"data:image/svg+xml,%3Csvg width='20' height='20' viewBox='0 0 20 20' xmlns='http://www.w3.org/2000/svg'%3E%3Cg fill='%2308172e' fill-opacity='0.03' fill-rule='evenodd'%3E%3Ccircle cx='3' cy='3' r='1'/%3E%3Ccircle cx='13' cy='13' r='1'/%3E%3C/g%3E%3C/svg%3E\")",
        "gold-gradient": "linear-gradient(135deg, #e9cf64 0%, #aa790d 100%)",
        "school-radial": "radial-gradient(circle at top, rgba(8, 23, 46, 0.04), transparent 50%), radial-gradient(circle at bottom right, rgba(210, 167, 88, 0.06), transparent 40%)"
      }
    }
  },
  plugins: []
};

export default config;
