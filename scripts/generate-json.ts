#!/usr/bin/env npx ts-node
/**
 * NeoMind Extensions JSON Generator
 *
 * This script generates all JSON files from a single source:
 * - Cargo.toml (extension metadata)
 * - frontend.json (frontend component definitions)
 *
 * Outputs:
 * - metadata.json (for each extension)
 * - index.json (marketplace index)
 */

import * as fs from 'fs';
import * as path from 'path';

interface CargoToml {
  package: {
    name: string;
    version: string;
    description?: string;
    authors?: string[];
    license?: string;
  };
}

interface FrontendComponent {
  name: string;
  type: string;
  displayName: string;
  description: string;
  icon: string;
  defaultSize: { width: number; height: number };
  minSize: { width: number; height: number };
  maxSize: { width: number; height: number };
  configSchema?: Record<string, any>;
  refreshable?: boolean;
  refreshInterval?: number;
}

interface FrontendJson {
  id: string;
  version: string;
  entrypoint: string;
  components: FrontendComponent[];
  i18n?: {
    defaultLanguage: string;
    supportedLanguages: string[];
  };
}

interface ExtensionMetadata {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  license: string;
  type: string;
  categories: string[];
  homepage: string;
  frontend?: {
    components: string[];
    entrypoint: string;
  };
}

interface MarketplaceExtension {
  id: string;
  name: string;
  description: string;
  version: string;
  author: string;
  license: string;
  type: string;
  categories: string[];
  homepage: string;
  metadata_url: string;
  frontend?: {
    components: string[];
    entrypoint: string;
  };
  builds: Record<string, { url: string }>;
}

const EXTENSIONS_DIR = path.join(__dirname, '..', 'extensions');
const MARKET_VERSION = '2.3.0';
const GITHUB_REPO = 'camthink-ai/NeoMind-Extensions';

// Parse Cargo.toml (simple TOML parser for basic format)
function parseCargoToml(tomlPath: string): CargoToml | null {
  if (!fs.existsSync(tomlPath)) return null;

  const content = fs.readFileSync(tomlPath, 'utf-8');
  const result: any = { package: {} };

  let currentSection = '';
  for (const line of content.split('\n')) {
    const trimmed = line.trim();
    if (trimmed.startsWith('[') && trimmed.endsWith(']')) {
      currentSection = trimmed.slice(1, -1);
      if (!result[currentSection]) result[currentSection] = {};
    } else if (trimmed.includes('=') && currentSection) {
      const [key, ...valueParts] = trimmed.split('=');
      const value = valueParts.join('=').trim();
      let parsed: any = value;
      if (value.startsWith('"') && value.endsWith('"')) {
        parsed = value.slice(1, -1);
      } else if (value.startsWith('[')) {
        try {
          parsed = JSON.parse(value.replace(/'/g, '"'));
        } catch (e) {
          parsed = value;
        }
      }
      result[currentSection][key.trim()] = parsed;
    }
  }

  return result;
}

// Generate metadata.json for an extension
function generateMetadata(extDir: string, cargo: CargoToml, frontend: FrontendJson | null): ExtensionMetadata {
  const extId = cargo.package.name;

  return {
    id: extId,
    name: cargo.package.description?.split('.')[0] || extId,
    version: cargo.package.version,
    description: cargo.package.description || '',
    author: cargo.package.authors?.join(', ') || 'NeoMind Team',
    license: cargo.package.license || 'MIT',
    type: 'native',
    categories: inferCategories(extId),
    homepage: `https://github.com/${GITHUB_REPO}/tree/main/extensions/${extId}`,
    frontend: frontend ? {
      components: frontend.components.map(c => c.name),
      entrypoint: frontend.entrypoint,
    } : undefined,
  };
}

// Infer categories from extension ID
function inferCategories(extId: string): string[] {
  if (extId.includes('yolo') || extId.includes('image')) return ['ai', 'vision', 'detection'];
  if (extId.includes('weather')) return ['weather'];
  if (extId.includes('video')) return ['video', 'streaming'];
  if (extId.includes('device')) return ['device-integration'];
  return ['utility'];
}

// Get platform suffix for URL
function getPlatformSuffix(platform: string, extId: string): string {
  const version = MARKET_VERSION;
  const extVersion = '2.0.0'; // Default extension version

  switch (platform) {
    case 'darwin-aarch64':
      return `${extId}-${extVersion}-darwin_aarch64.nep`;
    case 'darwin-x86_64':
      return `${extId}-${extVersion}-darwin_x86_64.nep`;
    case 'linux-x86_64':
      return `${extId}-${extVersion}-linux_amd64.nep`;
    case 'linux-aarch64':
      return `${extId}-${extVersion}-linux_arm64.nep`;
    case 'windows-x86_64':
      return `${extId}-${extVersion}-windows_amd64.nep`;
    default:
      return `${extId}-${extVersion}-${platform.replace('-', '_')}.nep`;
  }
}

// Main function
function main() {
  console.log('Generating extension JSON files...\n');

  const extensions = fs.readdirSync(EXTENSIONS_DIR).filter(f => {
    return fs.statSync(path.join(EXTENSIONS_DIR, f)).isDirectory() &&
           fs.existsSync(path.join(EXTENSIONS_DIR, f, 'Cargo.toml'));
  });

  const marketplaceExtensions: MarketplaceExtension[] = [];

  for (const extId of extensions) {
    const extDir = path.join(EXTENSIONS_DIR, extId);
    const cargoPath = path.join(extDir, 'Cargo.toml');
    const frontendPath = path.join(extDir, 'frontend', 'frontend.json');
    const metadataPath = path.join(extDir, 'metadata.json');

    console.log(`Processing ${extId}...`);

    // Parse Cargo.toml
    const cargo = parseCargoToml(cargoPath);
    if (!cargo?.package) {
      console.log(`  ⚠ Skipping ${extId}: No Cargo.toml package section`);
      continue;
    }

    // Parse frontend.json if exists
    let frontend: FrontendJson | null = null;
    if (fs.existsSync(frontendPath)) {
      try {
        frontend = JSON.parse(fs.readFileSync(frontendPath, 'utf-8'));
      } catch (e) {
        console.log(`  ⚠ Could not parse frontend.json`);
      }
    }

    // Generate metadata.json
    const metadata = generateMetadata(extDir, cargo, frontend);
    fs.writeFileSync(metadataPath, JSON.stringify(metadata, null, 2));
    console.log(`  ✓ Generated metadata.json`);

    // Add to marketplace index
    const platforms = ['darwin-aarch64', 'darwin-x86_64', 'linux-x86_64', 'linux-aarch64', 'windows-x86_64'];
    const builds: Record<string, { url: string }> = {};

    for (const platform of platforms) {
      builds[platform] = {
        url: `https://github.com/${GITHUB_REPO}/releases/download/v${MARKET_VERSION}/${getPlatformSuffix(platform, extId)}`
      };
    }

    marketplaceExtensions.push({
      id: metadata.id,
      name: metadata.name,
      description: metadata.description,
      version: metadata.version,
      author: metadata.author,
      license: metadata.license,
      type: metadata.type,
      categories: metadata.categories,
      homepage: metadata.homepage,
      metadata_url: `https://raw.githubusercontent.com/${GITHUB_REPO}/main/extensions/${extId}/metadata.json`,
      frontend: metadata.frontend,
      builds,
    });
  }

  // Generate index.json
  const indexPath = path.join(EXTENSIONS_DIR, 'index.json');
  const indexContent = {
    version: MARKET_VERSION,
    market_version: MARKET_VERSION,
    extensions: marketplaceExtensions,
  };

  fs.writeFileSync(indexPath, JSON.stringify(indexContent, null, 2));
  console.log(`\n✓ Generated index.json with ${marketplaceExtensions.length} extensions`);
  console.log(`\nDone! Version: ${MARKET_VERSION}`);
}

main();
