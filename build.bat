@echo off
cd /d "%~dp0"
echo [X-Matrix] Cleaning old build...
if exist "release" rmdir /s /q "release"
if exist "build" rmdir /s /q "build"
if exist "X-Matrix.spec" del /q "X-Matrix.spec"

echo [X-Matrix] Building app in directory mode (--onedir) for lightning fast startup...
pyinstaller --noconfirm --onedir --windowed --name "X-Matrix" --icon "%~dp0icon.ico" --distpath "release" --workpath "build" --add-data "%~dp0index.html;." main.py

echo.
if exist "release\X-Matrix\X-Matrix.exe" (
    echo [X-Matrix] Copying core assets...
    if exist "%~dp0xmatrix-core.exe" copy /y "%~dp0xmatrix-core.exe" "release\X-Matrix\" >nul
    if exist "%~dp0geoip.dat" copy /y "%~dp0geoip.dat" "release\X-Matrix\" >nul
    if exist "%~dp0geosite.dat" copy /y "%~dp0geosite.dat" "release\X-Matrix\" >nul

    echo [X-Matrix] Build success!
    echo [X-Matrix] Output location: release\X-Matrix\X-Matrix.exe
    explorer "release\X-Matrix"
) else (
    echo [X-Matrix] Build failed. Check errors above.
)
pause
