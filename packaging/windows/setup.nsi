Unicode true
!include "${CONFIG}"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"
!include "MUI2.nsh"
!include "nsDialogs.nsh"

Name "crosshair-rs"
OutFile "${SETUP_OUT}"
RequestExecutionLevel user
ManifestSupportedOS all
CRCCheck force
SetCompressor zlib
VIProductVersion "${PACKAGE_VERSION}"
VIAddVersionKey /LANG=1033 "ProductName" "crosshair-rs"
VIAddVersionKey /LANG=1033 "FileDescription" "crosshair-rs Setup"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "crosshair-rs contributors"
BrandingText "crosshair-rs"
!define MUI_WELCOMEPAGE_TEXT "Install crosshair-rs ${VERSION}."
!insertmacro MUI_PAGE_WELCOME
Page custom OptionsPage OptionsLeave
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TEXT "Installation complete."
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Launch crosshair-rs"
!define MUI_FINISHPAGE_RUN_FUNCTION LaunchApp
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "English"

Var Mode
Var DesktopShortcut
Var StartMenuShortcut
Var Startup
Var DesktopControl
Var StartMenuControl
Var StartupControl
Var ApplyOptions
Function .onInit
    StrCpy $Mode "install"
    StrCpy $DesktopShortcut 1
    StrCpy $StartMenuShortcut 1
    StrCpy $Startup 0
    StrCpy $ApplyOptions 1
    IfSilent 0 +4
    StrCpy $DesktopShortcut "keep"
    StrCpy $StartMenuShortcut "keep"
    StrCpy $ApplyOptions 0
    ${GetParameters} $0
    ClearErrors
    ${GetOptions} $0 "/DESKTOP=" $1
    ${IfNot} ${Errors}
        StrCpy $DesktopShortcut $1
        StrCpy $ApplyOptions 1
    ${EndIf}
    ClearErrors
    ${GetOptions} $0 "/STARTMENU=" $1
    ${IfNot} ${Errors}
        StrCpy $StartMenuShortcut $1
        StrCpy $ApplyOptions 1
    ${EndIf}
    ClearErrors
    ${GetOptions} $0 "/STARTUP=" $1
    ${IfNot} ${Errors}
        StrCpy $Startup $1
        StrCpy $ApplyOptions 1
    ${EndIf}
    ${If} $DesktopShortcut != 0
    ${AndIf} $DesktopShortcut != 1
    ${AndIf} $DesktopShortcut != "keep"
        SetErrorLevel 1
        Quit
    ${EndIf}
    ${If} $StartMenuShortcut != 0
    ${AndIf} $StartMenuShortcut != 1
    ${AndIf} $StartMenuShortcut != "keep"
        SetErrorLevel 1
        Quit
    ${EndIf}
    ${If} $Startup != 0
    ${AndIf} $Startup != 1
        SetErrorLevel 1
        Quit
    ${EndIf}
    ClearErrors
    ${GetOptions} $0 "/VERIFYONLY" $1
    ${IfNot} ${Errors}
        StrCpy $Mode "verify"
        SetSilent silent
    ${EndIf}
    ClearErrors
    ${GetOptions} $0 "/EXTRACT" $1
    ${IfNot} ${Errors}
        StrCpy $Mode "extract"
        SetSilent silent
    ${EndIf}
    ${IfNot} ${RunningX64}
        SetErrorLevel 1
        IfSilent +2
        MessageBox MB_OK|MB_ICONSTOP "crosshair-rs requires x64 Windows 11."
        Quit
    ${EndIf}
FunctionEnd

Function OptionsPage
    !insertmacro MUI_HEADER_TEXT "Options" "Choose shortcuts and startup options."
    nsDialogs::Create 1018
    Pop $0
    ${If} $0 == error
        Abort
    ${EndIf}
    ${NSD_CreateCheckbox} 0 8u 100% 12u "Create a desktop shortcut"
    Pop $DesktopControl
    ${NSD_SetState} $DesktopControl $DesktopShortcut
    ${NSD_CreateCheckbox} 0 30u 100% 12u "Create an additional Start menu shortcut"
    Pop $StartMenuControl
    ${NSD_SetState} $StartMenuControl $StartMenuShortcut
    ${NSD_CreateCheckbox} 0 52u 100% 12u "Launch at Windows sign-in"
    Pop $StartupControl
    ${NSD_SetState} $StartupControl $Startup
    nsDialogs::Show
FunctionEnd

Function OptionsLeave
    ${NSD_GetState} $DesktopControl $DesktopShortcut
    ${NSD_GetState} $StartMenuControl $StartMenuShortcut
    ${NSD_GetState} $StartupControl $Startup
FunctionEnd

Function LaunchApp
    nsExec::ExecToStack '"$PLUGINSDIR\install.exe" --launch'
    Pop $1
    Pop $2
    ${If} $1 != 0
        SetErrorLevel 1
        ${If} $1 == "error"
        ${OrIf} $1 == "timeout"
            MessageBox MB_OK|MB_ICONSTOP "crosshair-rs could not start. Open it from Start."
        ${EndIf}
    ${EndIf}
FunctionEnd

Section "Install"
    ${If} $Mode == "extract"
        ; The updater authenticates this EXE before requesting its payload.
        IfFileExists "$EXEDIR\package.msix" failed
        SetOutPath "$EXEDIR"
        SetOverwrite off
        ClearErrors
        File /oname=package.msix "${PAYLOAD}"
        IfErrors failed
        SetErrorLevel 0
        Quit
    ${EndIf}
    InitPluginsDir
    SetOutPath "$PLUGINSDIR"
    File /oname=package.msix "${PAYLOAD}"
    File /oname=Microsoft.VCLibs.x64.14.00.appx "${FRAMEWORK}"
    File /oname=install.exe "${INSTALL_HELPER}"
    StrCpy $0 "--install"
    IfSilent 0 +2
    StrCpy $0 "--install-silent"
    ${If} $Mode == "verify"
        StrCpy $0 "--verify"
    ${EndIf}
    ; The native helper requests UAC and reports Windows deployment errors itself.
    SetOutPath "$WINDIR\System32"
    nsExec::ExecToStack '"$PLUGINSDIR\install.exe" $0'
    Pop $1
    Pop $2
    ${If} $1 != 0
        DetailPrint "$2"
        Goto failed
    ${EndIf}
    ${If} $Mode == "install"
    ${AndIf} $ApplyOptions == 1
        StrCpy $0 "--options"
        IfSilent 0 +2
        StrCpy $0 "--options-silent"
        nsExec::ExecToStack '"$PLUGINSDIR\install.exe" $0 $DesktopShortcut $StartMenuShortcut $Startup'
        Pop $1
        Pop $2
        ${If} $1 != 0
            DetailPrint "$2"
            Goto failed
        ${EndIf}
    ${EndIf}
    SetErrorLevel 0
    Goto done
failed:
    SetErrorLevel 1
    Abort
done:
SectionEnd
