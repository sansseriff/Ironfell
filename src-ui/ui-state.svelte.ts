export class UIState {
    colorMode: boolean = $state(false);
    private mediaQuery: MediaQueryList | null = null;

    constructor() {
        this.initializeTheme();
    }

    setMode(value: boolean) {

        this.colorMode = value;

        // update page styling
        if (value) {
            console.log("Switching to dark mode");
            document.documentElement.classList.add("dark");
        } else {
            console.log("Switching to light mode");
            document.documentElement.classList.remove("dark");
        }

        // store the theme as a local override
        localStorage.theme = value ? "dark" : "light";

        // if the toggled-to theme matches the system defined theme, clear the local override
        // this effectively provides a way to override or revert to "automatic" setting mode
        if (
            window.matchMedia(`(prefers-color-scheme: ${localStorage.theme})`).matches
        ) {
            localStorage.removeItem("theme");
        }

        return value;
    }

    private initializeTheme() {
        const isDarkMode = window.matchMedia && 
            window.matchMedia("(prefers-color-scheme: dark)").matches;
        
        this.setMode(isDarkMode);
        this.setupSystemPreferenceListener();
    }

    private setupSystemPreferenceListener() {
        if (window.matchMedia) {
            this.mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
            
            const handleChange = (e: MediaQueryListEvent) => {
                if (!localStorage.theme) {
                    this.setMode(e.matches);
                }
            };

            this.mediaQuery.addEventListener('change', handleChange);
        }
    }
}

// export let ui_state = new UIState();



