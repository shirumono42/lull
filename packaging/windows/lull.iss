; Inno Setup script for the Windows installer.
; Installs per user (no admin) to %LOCALAPPDATA%\Programs\Lull and adds a
; Start menu entry, so launchers such as Raycast and Windows Search find Lull.
;
; Build:  set LULL_VERSION=0.3.3 && iscc packaging\windows\lull.iss
; Output: dist\Lull-windows-x64-setup.exe

#define AppVersion GetEnv("LULL_VERSION")
#if AppVersion == ""
  #define AppVersion "0.0.0"
#endif

[Setup]
; Fixed ID: lets new versions install over old ones and keeps one entry in Settings > Apps.
AppId={{2D7B8AFF-E0BB-4F20-8F61-0294E8028B51}
AppName=Lull
AppVersion={#AppVersion}
AppVerName=Lull {#AppVersion}
AppPublisher=shirumono42
AppPublisherURL=https://github.com/shirumono42/lull
AppSupportURL=https://github.com/shirumono42/lull/issues
DefaultDirName={localappdata}\Programs\Lull
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
OutputDir=..\..\dist
OutputBaseFilename=Lull-windows-x64-setup
SetupIconFile=..\..\assets\lull.ico
UninstallDisplayIcon={app}\Lull.exe
UninstallDisplayName=Lull
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Lull is closed in PrepareToInstall below instead of via Restart Manager prompts.
CloseApplications=no
ShowLanguageDialog=no
LanguageDetectionMethod=uilanguage

[Languages]
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
ja.StartupTask=Windows へのサインイン時に Lull を起動する
en.StartupTask=Start Lull when I sign in to Windows

[Tasks]
Name: "startup"; Description: "{cm:StartupTask}"; Flags: unchecked

[Files]
Source: "..\..\target\release\lull.exe"; DestDir: "{app}"; DestName: "Lull.exe"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Lull"; Filename: "{app}\Lull.exe"; Comment: "Noise, binaural beats and Pomodoro timer"
Name: "{userstartup}\Lull"; Filename: "{app}\Lull.exe"; Tasks: startup

[Run]
Filename: "{app}\Lull.exe"; Description: "{cm:LaunchProgram,Lull}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/IM Lull.exe /F"; Flags: runhidden; RunOnceId: "CloseLull"

[Code]
// A running Lull locks Lull.exe; close it so an update can replace the file.
// (Lull also closes older copies itself when the new one starts.)
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM Lull.exe /F', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(500);
  Result := '';
end;
