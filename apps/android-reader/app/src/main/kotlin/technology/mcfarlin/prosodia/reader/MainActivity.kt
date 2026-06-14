package technology.mcfarlin.prosodia.reader

import android.content.Context
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.*
import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import technology.mcfarlin.prosodia.actor.LiteRtVocalActor
import technology.mcfarlin.prosodia.director.LiteRtLmDirector
import technology.mcfarlin.prosodia.stage.*
import java.io.File
import java.io.FileOutputStream

// MARK: - Reader Theme Definitions

enum class ReaderTheme(
    val title: String,
    val backgroundColor: Color,
    val textColor: Color,
    val highlightColor: Color,
    val highlightBorderColor: Color,
    val cardBackgroundColor: Color,
    val accentColor: Color
) {
    DARK(
        title = "Slate Dark",
        backgroundColor = Color(0 PallidSlateDarkBg), // #0C0E12
        textColor = Color(0xFFEAEAEA),
        highlightColor = Color(0x289C27B0), // Purple opacity 0.15
        highlightBorderColor = Color(0x9C27B0FF.toInt()), // Purple border
        cardBackgroundColor = Color(0x1AFFFFFF), // Transparent white
        accentColor = Color(0xFFAB47BC)
    ),
    SEPIA(
        title = "Warm Sepia",
        backgroundColor = Color(0xFFFAF6EE),
        textColor = Color(0xFF382E1E),
        highlightColor = Color(0x33E0A96D), // Amber opacity 0.20
        highlightBorderColor = Color(0xCCE0A96D.toInt()),
        cardBackgroundColor = Color(0x12000000), // Transparent black
        accentColor = Color(0xFFD78C3D)
    ),
    LIGHT(
        title = "Cream Light",
        backgroundColor = Color(0xFFF9F9F6),
        textColor = Color(0xFF262626),
        highlightColor = Color(0x282196F3), // Blue opacity 0.15
        highlightBorderColor = Color(0x992196F3),
        cardBackgroundColor = Color(0x0F000000),
        accentColor = Color(0xFF1976D2)
    );

    companion object {
        const val PallidSlateDarkBg = 0xFF0C0E12
    }
}

// MARK: - Stub Implementations for Offline Fallback

class StubDirectorInference : uniffi.stage.DirectorInference {
    override fun annotate(passage: String): String {
        // Inject baseline director tags
        return "[V: 0.1 A: -0.1 T: 0.0] $passage"
    }
}

class StubVocalActor : uniffi.stage.VocalActor {
    override fun render(payload: String): List<Float> {
        // Return 0.1 seconds of mock audio (silence) at 24000Hz mono
        return FloatArray(2400).toList()
    }
}

// MARK: - ViewModel

class ReaderViewModel : ViewModel() {
    var bookTitle by mutableStateOf("Alice\'s Adventures in Wonderland")
        private set
    var chapters by mutableStateOf<List<BookChapter>>(emptyList())
        private set
    var selectedChapterIndex by mutableIntStateOf(0)
        private set
    var sentences by mutableStateOf<List<String>>(emptyList())
        private set

    // Playback States
    var isPlaying by mutableStateOf(false)
        private set
    var currentSentenceIndex by mutableStateOf<Int?>(null)
        private set
    var activeSentenceText by mutableStateOf("")
        private set
    var isModelAvailable by mutableStateOf(false)
        private set

    // UI Configuration States
    var fontSize by mutableFloatStateOf(18f)
    var selectedTheme by mutableStateOf(ReaderTheme.DARK)
    var playbackSpeed by mutableFloatStateOf(1.0f)
    var narrationMode by mutableStateOf(NarrationMode.SOLO)
    var isSettingsOpen by mutableStateOf(false)

    private var playbackController: PlaybackController? = null
    private val segmenter = uniffi.stage.SentenceSegmenter()

    init {
        loadDefaultSampleBook()
        checkModelsAvailability()
    }

    private fun checkModelsAvailability() {
        val storagePath = "/sdcard/Models"
        val directorModel = File(storagePath, "gemma-4-E2B-it.litertlm")
        val actorModel = File(storagePath, "styletts2_lite.tflite")
        isModelAvailable = directorModel.exists() && actorModel.exists()
    }

    private fun loadDefaultSampleBook() {
        chapters = listOf(
            BookChapter(
                0, "Chapter I: Down the Rabbit-Hole",
                "Alice was beginning to get very tired of sitting by her sister on the bank, and of having nothing to do: once or twice she had peeped into the book her sister was reading, but it had no pictures or conversations in it, ‘and what is the use of a book,’ thought Alice ‘without pictures or conversations?’\n\nSo she was considering in her own mind (as well as she could, for the hot day made her feel very sleepy and stupid), whether the pleasure of making a daisy-chain would be worth the trouble of getting up and picking the daisies, when suddenly a White Rabbit with pink eyes ran close by her."
            ),
            BookChapter(
                1, "Chapter II: The Pool of Tears",
                "‘Curiouser and curiouser!’ cried Alice (she was so much surprised, that for the moment she quite forgot how to speak good English); ‘now I’m opening out like the largest telescope that ever was! Good-bye, feet!’ (for when she looked down at her feet, they seemed to be almost out of sight, they were getting so far off)."
            )
        )
        selectChapter(0)
    }

    fun selectChapter(index: Int) {
        if (index < 0 || index >= chapters.size) return
        stopPlayback()
        selectedChapterIndex = index
        val rawText = chapters[index].text
        
        // Use Rust-backed SentenceSegmenter to split text into narration units
        sentences = segmenter.sentences(rawText)
        currentSentenceIndex = null
        activeSentenceText = ""
    }

    fun loadBookFromUri(context: Context, uri: Uri) {
        viewModelScope.launch {
            try {
                val name = getFileName(context, uri) ?: "Imported Book"
                val file = File(context.cacheDir, "imported_temp_book")
                if (file.exists()) file.delete()

                withContext(Dispatchers.IO) {
                    context.contentResolver.openInputStream(uri)?.use { input ->
                        FileOutputStream(file).use { output ->
                            input.copyTo(output)
                        }
                    }
                }

                if (name.endsWith(".epub", ignoreCase = true)) {
                    val epubChapters = uniffi.folioparser.parseEpub(file.absolutePath)
                    if (epubChapters.isNotEmpty()) {
                        bookTitle = name.removeSuffix(".epub")
                        chapters = epubChapters.mapIndexed { index, ch ->
                            BookChapter(index, ch.title, ch.text)
                        }
                        selectChapter(0)
                    }
                } else {
                    // Treat as plain text
                    val text = file.readText(Charsets.UTF_8)
                    bookTitle = name.removeSuffix(".txt")
                    val rawChapters = text.split("\u000C").map { it.trim() }.filter { it.isNotEmpty() }
                    chapters = if (rawChapters.isNotEmpty()) {
                        rawChapters.mapIndexed { index, chText ->
                            BookChapter(index, "Section ${index + 1}", chText)
                        }
                    } else {
                        listOf(BookChapter(0, "Book", text))
                    }
                    selectChapter(0)
                }
            } catch (e: Exception) {
                e.printStackTrace()
            }
        }
    }

    private fun getFileName(context: Context, uri: Uri): String? {
        var name: String? = null
        if (uri.scheme == "content") {
            context.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) {
                    val nameIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    if (nameIndex != -1) {
                        name = cursor.getString(nameIndex)
                    }
                }
            }
        }
        if (name == null) {
            name = uri.path?.let {
                val cut = it.lastIndexOf('/')
                if (cut != -1) it.substring(cut + 1) else it
            }
        }
        return name
    }

    fun togglePlayPause() {
        if (isPlaying) {
            pausePlayback()
        } else {
            startPlayback()
        }
    }

    private fun startPlayback() {
        if (playbackController != null) {
            viewModelScope.launch {
                playbackController?.resume()
                isPlaying = true
            }
            return
        }

        viewModelScope.launch {
            val doc = InMemoryBookDocument(chapters.map { it.text })
            
            // Model paths setup
            val storagePath = "/sdcard/Models"
            val directorModelFile = File(storagePath, "gemma-4-E2B-it.litertlm")
            val actorModelFile = File(storagePath, "styletts2_lite.tflite")
            val actorConfigFile = File(storagePath, "config.json")
            val voiceDir = File(storagePath, "voices")

            val director: uniffi.stage.DirectorInference = if (directorModelFile.exists()) {
                LiteRtLmDirector(directorModelFile.absolutePath, narrationMode)
            } else {
                StubDirectorInference()
            }

            val actor: uniffi.stage.VocalActor = if (actorModelFile.exists() && actorConfigFile.exists() && voiceDir.exists()) {
                LiteRtVocalActor(
                    modelPath = actorModelFile.absolutePath,
                    configPath = actorConfigFile.absolutePath,
                    voiceDirectoryPath = voiceDir.absolutePath
                )
            } else {
                StubVocalActor()
            }

            isPlaying = true

            // Run Android audio StageCoordinator output flow
            val controller = StageCoordinator.run(
                document = doc,
                grouping = uniffi.stage.NarrationGrouping.Sentence,
                director = director,
                actor = actor
            )
            playbackController = controller

            viewModelScope.launch(Dispatchers.Main) {
                controller.events.collectLatest { event ->
                    when (event) {
                        is PlaybackEvent.SentenceBegan -> {
                            currentSentenceIndex = event.index
                            if (event.index >= 0 && event.index < sentences.size) {
                                activeSentenceText = sentences[event.index]
                            }
                        }
                        is PlaybackEvent.Finished -> {
                            isPlaying = false
                            currentSentenceIndex = null
                            activeSentenceText = ""
                            playbackController = null
                        }
                        else -> {}
                    }
                }
            }
        }
    }

    private fun pausePlayback() {
        viewModelScope.launch {
            playbackController?.pause()
            isPlaying = false
        }
    }

    fun stopPlayback() {
        viewModelScope.launch {
            playbackController?.stop()
            playbackController = null
            isPlaying = false
            currentSentenceIndex = null
            activeSentenceText = ""
        }
    }

    override fun onCleared() {
        super.onCleared()
        stopPlayback()
        segmenter.close()
    }
}

// MARK: - UI Components

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            val viewModel = remember { ReaderViewModel() }
            ProsodiaReaderApp(viewModel)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProsodiaReaderApp(viewModel: ReaderViewModel) {
    val theme = viewModel.selectedTheme
    val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    val filePickerLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.GetContent()
    ) { uri: Uri? ->
        uri?.let { viewModel.loadBookFromUri(context, it) }
    }

    ModalNavigationDrawer(
        drawerState = drawerState,
        drawerContent = {
            ModalDrawerSheet(
                drawerContainerColor = theme.backgroundColor,
                modifier = Modifier.width(300.dp)
            ) {
                Text(
                    text = "Chapters",
                    modifier = Modifier.padding(16.dp),
                    fontSize = 20.sp,
                    fontWeight = FontWeight.Bold,
                    color = theme.textColor
                )
                Divider(color = theme.textColor.copy(alpha = 0.12f))
                LazyColumn(modifier = Modifier.fillMaxSize()) {
                    itemsIndexed(viewModel.chapters) { index, ch ->
                        NavigationDrawerItem(
                            label = {
                                Text(
                                    text = ch.title ?: "Chapter ${index + 1}",
                                    color = if (viewModel.selectedChapterIndex == index) theme.accentColor else theme.textColor,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis
                                )
                            },
                            selected = viewModel.selectedChapterIndex == index,
                            onClick = {
                                viewModel.selectChapter(index)
                                scope.launch { drawerState.close() }
                            },
                            colors = NavigationDrawerItemDefaults.colors(
                                selectedContainerColor = theme.accentColor.copy(alpha = 0.12f),
                                unselectedContainerColor = Color.Transparent
                            ),
                            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp)
                        )
                    }
                }
            }
        }
    ) {
        Scaffold(
            topBar = {
                CenterAlignedTopAppBar(
                    title = {
                        Text(
                            text = viewModel.bookTitle,
                            fontSize = 18.sp,
                            fontWeight = FontWeight.Bold,
                            color = theme.textColor,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis
                        )
                    },
                    navigationIcon = {
                        IconButton(onClick = { scope.launch { drawerState.open() } }) {
                            Icon(Icons.Default.Menu, contentDescription = "Menu", tint = theme.textColor)
                        }
                    },
                    actions = {
                        IconButton(onClick = { filePickerLauncher.launch("*/*") }) {
                            Icon(Icons.Default.FileOpen, contentDescription = "Import File", tint = theme.textColor)
                        }
                        IconButton(onClick = { viewModel.isSettingsOpen = !viewModel.isSettingsOpen }) {
                            Icon(Icons.Default.Settings, contentDescription = "Settings", tint = theme.textColor)
                        }
                    },
                    colors = TopAppBarDefaults.centerAlignedTopAppBarColors(
                        containerColor = theme.backgroundColor
                    )
                )
            },
            containerColor = theme.backgroundColor
        ) { paddingValues ->
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(paddingValues)
                    .background(
                        Brush.verticalGradient(
                            colors = listOf(
                                theme.backgroundColor,
                                theme.backgroundColor.copy(alpha = 0.95f)
                            )
                        )
                    )
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(horizontal = 24.dp)
                ) {
                    // Reading text pane
                    val listState = rememberLazyListState()
                    LaunchedEffect(viewModel.currentSentenceIndex) {
                        viewModel.currentSentenceIndex?.let { index ->
                            if (index >= 0 && index < viewModel.sentences.size) {
                                listState.animateScrollToItem(index)
                            }
                        }
                    }

                    LazyColumn(
                        state = listState,
                        modifier = Modifier
                            .weight(1f)
                            .fillMaxWidth()
                            .padding(vertical = 16.dp)
                    ) {
                        itemsIndexed(viewModel.sentences) { index, sentence ->
                            val isCurrent = viewModel.currentSentenceIndex == index
                            val highlightColor by animateColorAsState(
                                targetValue = if (isCurrent) theme.highlightColor else Color.Transparent,
                                label = "highlight"
                            )
                            val borderColor by animateColorAsState(
                                targetValue = if (isCurrent) theme.highlightBorderColor else Color.Transparent,
                                label = "border"
                            )

                            Box(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(vertical = 6.dp)
                                    .clip(RoundedCornerShape(8.dp))
                                    .background(highlightColor)
                                    .border(1.dp, borderColor, RoundedCornerShape(8.dp))
                                    .clickable { viewModel.selectChapter(viewModel.selectedChapterIndex) }
                                    .padding(12.dp)
                            ) {
                                Text(
                                    text = sentence,
                                    fontSize = viewModel.fontSize.sp,
                                    lineHeight = (viewModel.fontSize * 1.5).sp,
                                    color = theme.textColor,
                                    fontFamily = FontFamily.Serif
                                )
                            }
                        }
                    }

                    // Glassmorphic Control panel
                    GlassmorphicControlPanel(viewModel, theme)
                }

                // Settings Drawer overlay
                AnimatedVisibility(
                    visible = viewModel.isSettingsOpen,
                    enter = slideInVertically(initialOffsetY = { it }) + fadeIn(),
                    exit = slideOutVertically(targetOffsetY = { it }) + fadeOut(),
                    modifier = Modifier.align(Alignment.BottomCenter)
                ) {
                    SettingsPanel(viewModel, theme)
                }
            }
        }
    }
}

@Composable
fun GlassmorphicControlPanel(viewModel: ReaderViewModel, theme: ReaderTheme) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 24.dp)
            .shadow(16.dp, RoundedCornerShape(18.dp))
            .background(theme.cardBackgroundColor.copy(alpha = 0.08f), RoundedCornerShape(18.dp))
            .border(1.dp, theme.textColor.copy(alpha = 0.08f), RoundedCornerShape(18.dp))
            .padding(20.dp)
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier.fillMaxWidth()
        ) {
            if (viewModel.activeSentenceText.isNotEmpty()) {
                Text(
                    text = viewModel.activeSentenceText,
                    fontSize = 14.sp,
                    color = theme.textColor.copy(alpha = 0.7f),
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(bottom = 12.dp)
                )
            }

            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceEvenly,
                modifier = Modifier.fillMaxWidth()
            ) {
                IconButton(onClick = { viewModel.stopPlayback() }) {
                    Icon(Icons.Default.Stop, contentDescription = "Stop", tint = theme.textColor, modifier = Modifier.size(28.dp))
                }

                Box(
                    modifier = Modifier
                        .size(56.dp)
                        .clip(RoundedCornerShape(28.dp))
                        .background(theme.accentColor)
                        .clickable { viewModel.togglePlayPause() },
                    contentAlignment = Alignment.Center
                ) {
                    Icon(
                        imageVector = if (viewModel.isPlaying) Icons.Default.Pause else Icons.Default.PlayArrow,
                        contentDescription = "Play/Pause",
                        tint = Color.White,
                        modifier = Modifier.size(32.dp)
                    )
                }

                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        text = if (viewModel.isModelAvailable) "Neural TTS" else "Stub mode",
                        fontSize = 11.sp,
                        fontWeight = FontWeight.Bold,
                        color = if (viewModel.isModelAvailable) Color(0xFF4CAF50) else Color(0xFFFF9800)
                    )
                }
            }
        }
    }
}

@Composable
fun SettingsPanel(viewModel: ReaderViewModel, theme: ReaderTheme) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .shadow(24.dp, RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp))
            .background(theme.backgroundColor, RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp))
            .border(1.dp, theme.textColor.copy(alpha = 0.08f), RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp))
            .padding(24.dp)
    ) {
        Column(modifier = Modifier.fillMaxWidth()) {
            Row(
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth()
            ) {
                Text(text = "Narration Settings", fontSize = 18.sp, fontWeight = FontWeight.Bold, color = theme.textColor)
                IconButton(onClick = { viewModel.isSettingsOpen = false }) {
                    Icon(Icons.Default.Close, contentDescription = "Close", tint = theme.textColor)
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            // Typography Config
            Text(text = "Font Size: ${viewModel.fontSize.toInt()}sp", fontSize = 14.sp, color = theme.textColor.copy(alpha = 0.7f))
            Slider(
                value = viewModel.fontSize,
                onValueChange = { viewModel.fontSize = it },
                valueRange = 14f..28f,
                colors = SliderDefaults.colors(
                    thumbColor = theme.accentColor,
                    activeTrackColor = theme.accentColor
                )
            )

            Spacer(modifier = Modifier.height(12.dp))

            // Speed Config
            Text(text = "Speed: ${String.format("%.2fx", viewModel.playbackSpeed)}", fontSize = 14.sp, color = theme.textColor.copy(alpha = 0.7f))
            Slider(
                value = viewModel.playbackSpeed,
                onValueChange = { viewModel.playbackSpeed = it },
                valueRange = 0.5f..2.0f,
                colors = SliderDefaults.colors(
                    thumbColor = theme.accentColor,
                    activeTrackColor = theme.accentColor
                )
            )

            Spacer(modifier = Modifier.height(16.dp))

            // Theme Selector
            Text(text = "Theme", fontSize = 14.sp, color = theme.textColor.copy(alpha = 0.7f), modifier = Modifier.padding(bottom = 8.dp))
            Row(
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                modifier = Modifier.fillMaxWidth()
            ) {
                ReaderTheme.values().forEach { rTheme ->
                    Box(
                        modifier = Modifier
                            .weight(1f)
                            .height(40.dp)
                            .clip(RoundedCornerShape(8.dp))
                            .background(rTheme.backgroundColor)
                            .border(
                                width = if (viewModel.selectedTheme == rTheme) 2.dp else 1.dp,
                                color = if (viewModel.selectedTheme == rTheme) theme.accentColor else rTheme.textColor.copy(alpha = 0.15f),
                                shape = RoundedCornerShape(8.dp)
                            )
                            .clickable { viewModel.selectedTheme = rTheme },
                        contentAlignment = Alignment.Center
                    ) {
                        Text(
                            text = rTheme.title,
                            color = rTheme.textColor,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.SemiBold
                        )
                    }
                }
            }
        }
    }
}
