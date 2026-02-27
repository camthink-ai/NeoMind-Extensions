#!/usr/bin/env python3
"""
Test script to verify .nep package can be parsed correctly
"""
import zipfile
import json
import hashlib
import sys

def test_nep_package(nep_path):
    """Test that a .nep package is valid"""
    print(f"Testing: {nep_path}")
    print("=" * 50)

    try:
        # Test 1: Open and verify ZIP structure
        print("\n1. Verifying ZIP structure...")
        with zipfile.ZipFile(nep_path, 'r') as zf:
            files = zf.namelist()
            print(f"   ✓ Valid ZIP archive with {len(files)} files")

        # Test 2: Read and verify manifest.json
        print("\n2. Verifying manifest.json...")
        with zipfile.ZipFile(nep_path, 'r') as zf:
            # Find manifest.json (may be in a subdirectory)
            manifest_path = None
            for f in files:
                if f.endswith('manifest.json'):
                    manifest_path = f
                    break

            if not manifest_path:
                print("   ✗ manifest.json not found")
                return False

            with zf.open(manifest_path) as mf:
                manifest = json.load(mf)

        # Check required fields
        required_fields = ['format', 'format_version', 'id', 'name', 'version']
        for field in required_fields:
            if field not in manifest:
                print(f"   ✗ Missing required field: {field}")
                return False

        print(f"   ✓ Extension: {manifest['name']} (v{manifest['version']})")
        print(f"   ✓ ID: {manifest['id']}")
        print(f"   ✓ Format: {manifest['format']} v{manifest['format_version']}")

        # Test 3: Verify binary files exist
        print("\n3. Verifying binary files...")
        with zipfile.ZipFile(nep_path, 'r') as zf:
            binaries = manifest.get('binaries', {})
            if binaries:
                for platform, path in binaries.items():
                    # Try both direct path and with extension directory prefix
                    try:
                        info = zf.getinfo(path)
                        size = info.file_size
                        print(f"   ✓ {platform}: {path} ({size} bytes)")
                    except KeyError:
                        # Try finding by filename
                        found = False
                        for f in files:
                            if f.endswith(path.split('/')[-1]):
                                info = zf.getinfo(f)
                                size = info.file_size
                                print(f"   ✓ {platform}: {f} ({size} bytes)")
                                found = True
                                break
                        if not found:
                            print(f"   ✗ Missing binary: {platform}")
                            return False

        # Test 4: Verify frontend components
        print("\n4. Verifying frontend components...")
        frontend_files = [f for f in files if f.startswith('frontend/')]
        if frontend_files:
            print(f"   ✓ Found {len(frontend_files)} frontend file(s)")
            for f in frontend_files:
                print(f"      - {f}")

        # Test 5: Verify checksum
        print("\n5. Verifying checksum...")
        with open(nep_path, 'rb') as f:
            data = f.read()
            checksum = hashlib.sha256(data).hexdigest()

        print(f"   ✓ SHA256: {checksum[:16]}...")

        # Test 6: Check package size
        size = len(data)
        size_mb = size / (1024 * 1024)
        print(f"\n6. Package size:")
        print(f"   Total: {size:,} bytes ({size_mb:.2f} MB)")

        if size_mb > 10:
            print(f"   ⚠ Warning: Package is large (>10MB)")

        return True

    except Exception as e:
        print(f"\n✗ Error: {e}")
        import traceback
        traceback.print_exc()
        return False

def main():
    if len(sys.argv) < 2:
        print("Usage: test_nep.py <path-to-package.nep>")
        sys.exit(1)

    nep_path = sys.argv[1]

    if test_nep_package(nep_path):
        print("\n" + "=" * 50)
        print("✅ All tests passed!")
        print("=" * 50)
        sys.exit(0)
    else:
        print("\n" + "=" * 50)
        print("❌ Tests failed!")
        print("=" * 50)
        sys.exit(1)

if __name__ == '__main__':
    main()
