; Build through tools/package.ps1 so the EXE, installer and update manifest agree.
#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef SourceExe
  #error SourceExe is required
#endif
#ifndef OutputPath
  #error OutputPath is required
#endif
#ifndef AppIdValue
  #define AppIdValue "{{8AE7364D-46F2-4B6E-9270-B2702A3C9275}"
#endif
#ifndef AppNameValue
  #define AppNameValue "Edot"
#endif
#ifndef RegistryKey
  #define RegistryKey "Software\Edoli\Edot"
#endif
#ifndef ReleaseRepository
  #define ReleaseRepository "edoli/Edot"
#endif

[Setup]
AppId={#AppIdValue}
AppName={#AppNameValue}
AppVersion={#AppVersion}
AppPublisher=Edoli
AppPublisherURL=https://github.com/{#ReleaseRepository}
AppSupportURL=https://github.com/{#ReleaseRepository}/issues
AppUpdatesURL=https://github.com/{#ReleaseRepository}/releases
DefaultDirName={autopf}\Edot
DefaultGroupName={#AppNameValue}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
UsePreviousPrivileges=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputPath}
OutputBaseFilename=Edot-Setup-{#AppVersion}-windows-x64
SetupIconFile=..\assets\app-icon\edot.ico
UninstallDisplayIcon={app}\edot.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=no
CloseApplicationsFilter=edot.exe
RestartApplications=no
SetupLogging=yes

[Languages]
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "edot.exe"; Flags: ignoreversion

[Registry]
Root: HKA; Subkey: "{#RegistryKey}"; ValueType: string; ValueName: "InstallLocation"; ValueData: "{app}"; Flags: uninsdeletekeyifempty uninsdeletevalue

[Icons]
Name: "{autoprograms}\{#AppNameValue}"; Filename: "{app}\edot.exe"
Name: "{autodesktop}\{#AppNameValue}"; Filename: "{app}\edot.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\edot.exe"; Description: "{cm:LaunchProgram,Edot}"; Flags: nowait postinstall skipifsilent runasoriginaluser

; No AppData, Space or user files are installed or deleted by Setup/Uninstall.

[Code]
function CreateFileW(FileName: string; DesiredAccess, ShareMode: Cardinal;
  SecurityAttributes: Integer; CreationDisposition, FlagsAndAttributes: Cardinal;
  TemplateFile: THandle): THandle;
  external 'CreateFileW@kernel32.dll stdcall';
function CloseHandle(Handle: THandle): Boolean;
  external 'CloseHandle@kernel32.dll stdcall';

function PrepareToInstall(var NeedsRestart: Boolean): string;
var
  Handle: THandle;
  ExePath: string;
begin
  Result := '';
  ExePath := ExpandConstant('{app}\edot.exe');
  if FileExists(ExePath) then begin
    { A loaded executable cannot be opened for writing. Never force-close editors. }
    Handle := CreateFileW(ExePath, $40000000, 7, 0, 3, $80, 0);
    if Handle = THandle(-1) then
      Result := 'Edot을 사용하는 모든 창을 닫은 뒤 다시 시도하세요.';
    if Handle <> THandle(-1) then
      CloseHandle(Handle);
  end;
end;
