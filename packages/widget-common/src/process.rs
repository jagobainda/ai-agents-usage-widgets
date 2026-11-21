use std::env;
use std::ffi::OsStr;
use std::io::{self, PipeReader};
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use std::time::Duration;

use crate::utils::encode_wide;
use windows_sys::Win32::Foundation::{
    HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_FAILED, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

pub struct HiddenProcess {
    main: OwnedHandle,
    job: Mutex<Option<OwnedHandle>>,
}

impl HiddenProcess {
    pub fn spawn(
        executable: &Path,
        args: &[&str],
        child_stdin: impl AsRawHandle,
        env_override: Option<(&str, &str)>,
    ) -> io::Result<(Self, PipeReader, PipeReader)> {
        let (stdout, child_stdout) = io::pipe()?;
        let (stderr, child_stderr) = io::pipe()?;
        let child_pipes = [
            child_stdin.as_raw_handle(),
            child_stdout.as_raw_handle(),
            child_stderr.as_raw_handle(),
        ];
        for pipe in child_pipes {
            // SAFETY: Each pipe end is owned locally and the parent ends remain non-inheritable.
            check(unsafe { SetHandleInformation(pipe, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) })?;
        }

        let job = kill_on_close_job()?;
        let jobs = [job.as_raw_handle()];
        let mut attributes = ProcessAttributes::new()?;
        attributes.set(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &child_pipes)?;
        attributes.set(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
        let startup = STARTUPINFOEXW {
            StartupInfo: windows_sys::Win32::System::Threading::STARTUPINFOW {
                cb: size_of::<STARTUPINFOEXW>() as u32,
                dwFlags: STARTF_USESTDHANDLES,
                hStdInput: child_pipes[0],
                hStdOutput: child_pipes[1],
                hStdError: child_pipes[2],
                ..Default::default()
            },
            lpAttributeList: attributes.list,
        };
        let (program, mut arguments) = command_line(executable, args)?;
        let environment = env_override.map(environment_block);
        let mut process = PROCESS_INFORMATION::default();
        // SAFETY: The startup data, pipes and job outlive CreateProcessW. Windows assigns
        // the job before the launcher can create children.
        check(unsafe {
            CreateProcessW(
                program.as_ptr(),
                arguments.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                environment
                    .as_ref()
                    .map_or(null(), |block| block.as_ptr().cast()),
                null(),
                &startup.StartupInfo,
                &mut process,
            )
        })?;

        // SAFETY: On success CreateProcessW returns owned process and thread handles.
        let main = unsafe { OwnedHandle::from_raw_handle(process.hProcess) };
        drop(unsafe { OwnedHandle::from_raw_handle(process.hThread) });
        Ok((
            Self {
                main,
                job: Mutex::new(Some(job)),
            },
            stdout,
            stderr,
        ))
    }

    pub fn is_running(&self) -> bool {
        self.wait(Duration::ZERO).is_ok_and(|exited| !exited)
    }

    pub fn wait(&self, timeout: Duration) -> io::Result<bool> {
        wait(&self.main, timeout)
    }

    pub fn exit_status(&self) -> io::Result<ExitStatus> {
        let mut code = 0;
        // SAFETY: The process handle is owned and the result points to a writable DWORD.
        check(unsafe { GetExitCodeProcess(self.main.as_raw_handle(), &mut code) })?;
        Ok(ExitStatus::from_raw(code))
    }

    pub fn terminate_tree(&self) {
        drop(self.job.lock().unwrap().take());
    }
}

impl AsHandle for HiddenProcess {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.main.as_handle()
    }
}

fn command_line(executable: &Path, args: &[&str]) -> io::Result<(Vec<u16>, Vec<u16>)> {
    let executable = std::path::absolute(executable)?;
    let path = encode_wide(executable.as_os_str());
    let units = &path[..path.len() - 1];
    if units.contains(&0)
        || units.contains(&u16::from(b'"'))
        || args.iter().any(|arg| arg.contains('\0'))
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    if executable.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
    }) {
        let system_root = env::var_os("SystemRoot").ok_or(io::ErrorKind::NotFound)?;
        let program = PathBuf::from(system_root).join(r"System32\cmd.exe");
        let mut arguments: Vec<_> = "cmd.exe /d /e:on /v:off /s /c \"\""
            .encode_utf16()
            .collect();
        for &unit in units {
            if unit == u16::from(b'%') {
                arguments.extend("%%cd:~,".encode_utf16());
            }
            arguments.push(unit);
        }
        arguments.push(u16::from(b'"'));
        for arg in args {
            // Batch launchers receive only the fixed CLI arguments and fixture modes.
            if !arg
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_/".contains(&b))
                || arg.is_empty()
            {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            arguments.push(u16::from(b' '));
            arguments.extend(arg.encode_utf16());
        }
        arguments.extend([u16::from(b'"'), 0]);
        return Ok((encode_wide(program.as_os_str()), arguments));
    }
    let mut arguments = vec![u16::from(b'"')];
    arguments.extend(units);
    arguments.push(u16::from(b'"'));
    for arg in args {
        arguments.push(u16::from(b' '));
        append_argument(&mut arguments, arg);
    }
    arguments.push(0);
    Ok((path, arguments))
}

fn append_argument(command: &mut Vec<u16>, arg: &str) {
    let quote = arg.is_empty() || arg.contains([' ', '\t']);
    if quote {
        command.push(u16::from(b'"'));
    }
    let mut backslashes = 0;
    for unit in arg.encode_utf16() {
        if unit == u16::from(b'\\') {
            backslashes += 1;
            continue;
        }
        if unit == u16::from(b'"') {
            backslashes = backslashes * 2 + 1;
        }
        command.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes));
        command.push(unit);
        backslashes = 0;
    }
    command.extend(std::iter::repeat_n(
        u16::from(b'\\'),
        if quote { backslashes * 2 } else { backslashes },
    ));
    if quote {
        command.push(u16::from(b'"'));
    }
}

fn environment_block((key, value): (&str, &str)) -> Vec<u16> {
    let mut entries: Vec<_> = env::vars_os()
        .filter(|(name, _)| !name.eq_ignore_ascii_case(OsStr::new(key)))
        .collect();
    entries.push((key.into(), value.into()));
    entries.sort_by_cached_key(|(name, _)| name.to_string_lossy().to_uppercase());
    let mut block = Vec::new();
    for (name, value) in entries {
        let mut name = encode_wide(name);
        *name.last_mut().unwrap() = u16::from(b'=');
        block.extend(name);
        block.extend(encode_wide(value));
    }
    block.push(0);
    block
}

pub fn wait(handle: &impl AsRawHandle, timeout: Duration) -> io::Result<bool> {
    let millis = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
    // SAFETY: The process or thread handle remains owned for the duration of the wait.
    match unsafe { WaitForSingleObject(handle.as_raw_handle(), millis) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_FAILED => Err(io::Error::last_os_error()),
        _ => Ok(false),
    }
}

fn kill_on_close_job() -> io::Result<OwnedHandle> {
    // SAFETY: Null security attributes and name create a private, non-inheritable job.
    let handle = unsafe { CreateJobObjectW(null(), null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateJobObjectW returned a new owned handle.
    let job = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: The info class matches the buffer type and length.
    check(unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of_val(&limits) as u32,
        )
    })?;
    Ok(job)
}

struct ProcessAttributes {
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
    _buffer: Vec<usize>,
}

impl ProcessAttributes {
    fn new() -> io::Result<Self> {
        let mut size = 0;
        // SAFETY: The documented sizing call uses a null list and a valid size pointer.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; size.div_ceil(size_of::<usize>())];
        let list = buffer.as_mut_ptr().cast();
        // SAFETY: The buffer is aligned and large enough for the initialized attribute list.
        check(unsafe { InitializeProcThreadAttributeList(list, 2, 0, &mut size) })?;
        Ok(Self {
            list,
            _buffer: buffer,
        })
    }

    fn set(&mut self, key: u32, handles: &[HANDLE]) -> io::Result<()> {
        // SAFETY: Callers keep each handle array alive until both the spawn and list deletion.
        check(unsafe {
            UpdateProcThreadAttribute(
                self.list,
                0,
                key as usize,
                handles.as_ptr().cast(),
                size_of_val(handles),
                null_mut(),
                null(),
            )
        })
    }
}

impl Drop for ProcessAttributes {
    fn drop(&mut self) {
        // SAFETY: The list is initialized and the backing buffer drops after deletion.
        unsafe { DeleteProcThreadAttributeList(self.list) };
    }
}

fn check(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
