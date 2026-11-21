Unicode true
!include "${CONFIG}"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"
!include "MUI2.nsh"
!include "nsDialogs.nsh"

Name "${PRODUCT_NAME}"
OutFile "${SETUP_OUT}"
RequestExecutionLevel user
ManifestSupportedOS all
CRCCheck force
SetCompressor zlib
VIProductVersion "${PACKAGE_VERSION}"
VIAddVersionKey /LANG=1033 "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey /LANG=1033 "FileDescription" "${PRODUCT_NAME} Setup"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "crosshair-rs contributors"
BrandingText "crosshair-rs"
!define MUI_WELCOMEPAGE_TEXT "Install ${PRODUCT_NAME} ${VERSION}.$\r$\n$\r$\nUpdating? Save your edits and choose Quit from the crosshair-rs tray icon. Setup keeps your settings."
!insertmacro MUI_PAGE_WELCOME
Page custom OptionsPage OptionsLeave
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TEXT "Installation complete."
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Launch ${PRODUCT_NAME}"
!define MUI_FINISHPAGE_RUN_FUNCTION LaunchApp
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "English"

Var Mode
Var DesktopShortcut
Var StartMenuShortcut
Var Startup
Var DesktopControl
Var StartupControl
Var FailureCode
Function .onInit
    StrCpy $FailureCode 1
    StrCpy $Mode "install"
    StrCpy $DesktopShortcut "keep"
    StrCpy $StartMenuShortcut "keep"
    StrCpy $Startup 0
    ${GetParameters} $0
    ClearErrors
    ${GetOptions} $0 "/DESKTOP=" $1
    ${IfNot} ${Errors}
        StrCpy $DesktopShortcut $1
    ${EndIf}
    ClearErrors
    ${GetOptions} $0 "/STARTMENU=" $1
    ${IfNot} ${Errors}
        ; Keep explicit legacy switches working. Interactive setup uses the packaged Start entry.
        StrCpy $StartMenuShortcut $1
        ${If} $1 != 0
        ${AndIf} $1 != 1
        ${AndIf} $1 != "keep"
            SetErrorLevel 1
            Quit
        ${EndIf}
    ${EndIf}
    ClearErrors
    ${GetOptions} $0 "/STARTUP=" $1
    ${IfNot} ${Errors}
        StrCpy $Startup $1
    ${EndIf}
    ${If} $DesktopShortcut != 0
    ${AndIf} $DesktopShortcut != 1
    ${AndIf} $DesktopShortcut != "keep"
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
    ${If} $DesktopShortcut == "keep"
        InitPluginsDir
        SetOutPath "$PLUGINSDIR"
        File /oname=install.exe "${INSTALL_HELPER}"
        SetOutPath "$WINDIR\System32"
        nsExec::ExecToStack '"$PLUGINSDIR\install.exe" --shortcut-defaults'
        Pop $0
        Pop $1
        ${If} $0 == 0
        ${OrIf} $0 == 1
            StrCpy $DesktopShortcut $0
        ${EndIf}
    ${EndIf}
    nsDialogs::Create 1018
    Pop $0
    ${If} $0 == error
        Abort
    ${EndIf}
    ${NSD_CreateCheckbox} 0 8u 100% 12u "Create a desktop shortcut"
    Pop $DesktopControl
    ${If} $DesktopShortcut != "keep"
        ${NSD_SetState} $DesktopControl $DesktopShortcut
    ${EndIf}
    ${NSD_CreateCheckbox} 0 30u 100% 12u "Enable launch at Windows sign-in"
    Pop $StartupControl
    ${NSD_SetState} $StartupControl $Startup
    nsDialogs::Show
FunctionEnd

Function OptionsLeave
    ${NSD_GetState} $DesktopControl $0
    ${If} $DesktopShortcut != "keep"
    ${OrIf} $0 == 1
        StrCpy $DesktopShortcut $0
    ${EndIf}
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
        ${If} $1 != "error"
        ${AndIf} $1 != "timeout"
            StrCpy $FailureCode $1
        ${EndIf}
        Goto failed
    ${EndIf}
    ${If} $Mode == "install"
        ${If} $DesktopShortcut == "keep"
        ${AndIf} $StartMenuShortcut == "keep"
        ${AndIf} $Startup == 0
            Goto success
        ${EndIf}
        StrCpy $0 "--options"
        IfSilent 0 +2
        StrCpy $0 "--options-silent"
        nsExec::ExecToStack '"$PLUGINSDIR\install.exe" $0 $DesktopShortcut $StartMenuShortcut $Startup'
        Pop $1
        Pop $2
        ${If} $1 != 0
            DetailPrint "$2"
            ${If} $1 != "error"
            ${AndIf} $1 != "timeout"
                StrCpy $FailureCode $1
            ${EndIf}
            Goto failed
        ${EndIf}
    ${EndIf}
success:
    SetErrorLevel 0
    Goto done
failed:
    SetErrorLevel $FailureCode
    Abort
done:
SectionEnd
