; 悬浮球在 Windows 上把设置、历史和界面缓存都放在「安装目录\data」（见 tauri.windows.conf.json），
; API Key 放在 Windows 凭据管理器。打包工具自带的卸载程序只认 AppData 里的数据，这里补上。

!macro NSIS_HOOK_PREUNINSTALL
  ; 静默卸载没有界面可勾，/CLEAN 等于勾选「删除应用程序数据」
  ${GetOptions} $CMDLINE "/CLEAN" $0
  ${IfNot} ${Errors}
    StrCpy $DeleteAppDataCheckboxState 1
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; 升级时（/UPDATE）什么都不动
  ${If} $UpdateMode <> 1
    ; 开机启动的另一半记录（任务管理器里的启用状态）；Run 那一半打包工具已经删了
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${PRODUCTNAME}"
    ${If} $DeleteAppDataCheckboxState = 1
      ; 凭据管理器里的 API Key。目标名是 keyring 库定的「账户名.服务名」，见 src/config.rs；1 = 通用凭据
      System::Call 'advapi32::CredDeleteW(w "api_key.${BUNDLEID}", i 1, i 0)'
      ; 确认是悬浮球自己的 data 才删，免得误删同一目录下别的东西
      ${If} ${FileExists} "$INSTDIR\data\history.db"
      ${OrIf} ${FileExists} "$INSTDIR\data\EBWebView\*.*"
        ; 小球刚被卸载程序关掉时，它的界面进程可能还占着缓存文件一小会儿，所以最多试 10 秒
        ${For} $1 1 20
          RMDir /r "$INSTDIR\data"
          ${IfNot} ${FileExists} "$INSTDIR\data\*.*"
            ${ExitFor}
          ${EndIf}
          Sleep 500
        ${Next}
      ${EndIf}
      ; 打包工具之前删安装目录时 data 还在，没删掉；现在空了才会被删
      RMDir "$INSTDIR"
    ${EndIf}
  ${EndIf}
!macroend
