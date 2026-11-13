Unicode true
!include "${CONFIG}"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"
!include "MUI2.nsh"

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
!define MUI_WELCOMEPAGE_TEXT "Install crosshair-rs ${VERSION} and its required Windows component.$\r$\n$\r$\nWindows will ask for administrator approval. Your existing crosshairs and settings are kept."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TEXT "crosshair-rs is installed. Open it from Start."
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "English"

Var Mode
Function .onInit
    StrCpy $Mode "install"
    ${GetParameters} $0
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
    SetErrorLevel 0
    Goto done
failed:
    SetErrorLevel 1
    Abort
done:
SectionEnd
