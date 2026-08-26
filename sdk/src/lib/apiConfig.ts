/**
 * API configuration service that manages endpoints for both app and platform APIs
 */

export type ApiContext = "app" | "platform";

export interface ApiEndpoint {
  baseUrl: string;
  context: ApiContext;
}

/**
 * ApiConfig service that manages URL configuration for both contexts
 */
class ApiConfigService {
  private _appApiUrl: string = "";
  private _platformApiUrl: string = "";

  /**
   * Configure the API URLs for both app and platform contexts
   */
  configure(appApiUrl: string, platformApiUrl: string) {
    this._appApiUrl = appApiUrl;
    this._platformApiUrl = platformApiUrl;
  }

  /**
   * Get the platform API URL
   */
  get platformApiUrl(): string {
    return this._platformApiUrl;
  }

  /**
   * Get the app API URL
   */
  get appApiUrl(): string {
    return this._appApiUrl;
  }

  /**
   * Determine if a path is for the platform context
   */
  isPlatformPath(path: string): boolean {
    return path.includes("/platform/");
  }

  /**
   * Get the API endpoint for a given path
   */
  resolveEndpoint(path: string): ApiEndpoint {
    const isPlatform = this.isPlatformPath(path);

    return {
      baseUrl: isPlatform ? this._platformApiUrl : this._appApiUrl,
      context: isPlatform ? "platform" : "app"
    };
  }

  /**
   * Build a complete URL for an API path
   */
  buildUrl(path: string): string {
    // If the path is already a full URL, return it unchanged
    if (path.startsWith("http")) {
      return path;
    }

    const endpoint = this.resolveEndpoint(path);
    const baseUrl = endpoint.baseUrl.endsWith("/")
      ? endpoint.baseUrl.slice(0, -1)
      : endpoint.baseUrl;
    const pathWithSlash = path.startsWith("/") ? path : `/${path}`;
    return `${baseUrl}${pathWithSlash}`;
  }

  /**
   * Get the appropriate refresh token function name for a given path
   */
  getRefreshFunction(path: string): "platformRefreshToken" | "refreshToken" {
    return this.isPlatformPath(path) ? "platformRefreshToken" : "refreshToken";
  }
}

// Create a singleton instance
export const apiConfig = new ApiConfigService();
